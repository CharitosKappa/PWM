//! Face ID / Touch ID unlock through the client (decision D45): the blob the
//! app keeps and the key it wraps with a hardware key.

use pwm_client::{Client, ClientError, MemoryStore, Signer, Tokens, new_id};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, Store, VaultRecord};
use pwm_vault::{Item, ItemId, ItemKind};

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";

fn tokens() -> Tokens {
    Tokens::new("pwma_x".into(), "pwmr_x".into())
}

/// An unlocked client with one vault and one note; its Secret Key and vault.
fn unlocked() -> (Client<MemoryStore>, String, String) {
    let account_id = *ItemId::generate().unwrap().as_bytes();
    let new = create_account(PASSWORD, &account_id, KdfParams::MOBILE_FLOOR).unwrap();
    let mut store = MemoryStore::new();
    store
        .put_account(&AccountRecord {
            account_id,
            salt: new.salt,
            kdf_params: new.params,
            sealed_keyset: new.sealed_keyset,
        })
        .unwrap();
    let vault_id = *ItemId::generate().unwrap().as_bytes();
    let wrapped = new
        .keyset
        .wrap_vault_key(&SymmetricKey::generate().unwrap(), &vault_id)
        .unwrap();
    store
        .put_vault(&VaultRecord {
            vault_id,
            wrapped_key: wrapped,
            sync_cursor: 0,
        })
        .unwrap();
    let secret_key = new.secret_key.to_text().expose_secret().clone();
    let mut client = Client::new(store);
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    let vault = ItemId::from_bytes(vault_id).to_string();
    let id = ItemId::generate().unwrap();
    let note = Item::new(ItemKind::Note, "Wi-Fi", T0)
        .to_json_with_id(&id)
        .unwrap();
    client.save_item(&vault, note.expose_secret(), T0).unwrap();
    (client, secret_key, vault)
}

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> &'static str {
    result.unwrap_err().code()
}

#[test]
fn face_id_unlocks_without_the_master_password() {
    let (mut client, _, vault) = unlocked();
    let device = new_id().unwrap();
    let unlock = client.export_biometric_unlock(&device).unwrap();
    assert_eq!(unlock.blob[0], 1, "versioned");
    client.lock();
    assert_eq!(code(client.items(&vault)), "locked");

    client
        .unlock_biometric(&unlock.blob, unlock.key.expose_secret(), &device, tokens())
        .unwrap();
    assert!(client.is_unlocked());
    assert_eq!(client.items(&vault).unwrap().len(), 1);
    assert!(!format!("{unlock:?}").contains(&format!("{:?}", unlock.key.expose_secret())));
}

#[test]
fn only_its_own_key_device_and_account_open_it() {
    let (mut client, _, _) = unlocked();
    let device = new_id().unwrap();
    let unlock = client.export_biometric_unlock(&device).unwrap();
    let (other_client, _, _) = unlocked();
    let other = other_client.export_biometric_unlock(&device).unwrap();
    client.lock();

    let mut changed_version = unlock.blob.clone();
    changed_version[0] = 2;
    let wrong_key = [7u8; 32];
    let other_device = new_id().unwrap();
    for (blob, key, device) in [
        (&unlock.blob[..], &wrong_key[..], device.as_str()),
        (
            &unlock.blob[..],
            &unlock.key.expose_secret()[..],
            other_device.as_str(),
        ),
        (
            &other.blob[..],
            &other.key.expose_secret()[..],
            device.as_str(),
        ),
        (
            &changed_version[..],
            &unlock.key.expose_secret()[..],
            device.as_str(),
        ),
        (
            &unlock.blob[..10],
            &unlock.key.expose_secret()[..],
            device.as_str(),
        ),
        (
            &unlock.blob[..],
            &unlock.key.expose_secret()[..31],
            device.as_str(),
        ),
    ] {
        assert_eq!(
            code(client.unlock_biometric(blob, key, device, tokens())),
            "biometric_unlock_failed"
        );
        assert!(!client.is_unlocked());
    }
    assert_eq!(
        code(client.unlock_biometric(&unlock.blob, &wrong_key, "not-an-id", tokens())),
        "invalid_id"
    );
}

#[test]
fn it_needs_an_unlocked_vault_to_make() {
    let (mut client, secret_key, _) = unlocked();
    client.lock();
    assert_eq!(
        code(client.export_biometric_unlock(&new_id().unwrap())),
        "locked"
    );
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    assert!(client.export_biometric_unlock(&new_id().unwrap()).is_ok());
}

// --- Windows Hello (D68): the unlock key sealed under a signature's key ---

fn signature(byte: u8) -> Vec<u8> {
    vec![byte; 256]
}

#[test]
fn windows_hello_unlocks_with_the_same_signature_and_remakes_the_blob() {
    let (mut client, _, vault) = unlocked();
    let device = new_id().unwrap();
    let challenge = pwm_client::windows_hello_challenge().unwrap();
    let hello = client
        .export_windows_hello(
            Signer::WindowsHello,
            &device,
            challenge.expose_secret(),
            &signature(1),
        )
        .unwrap();
    assert_eq!(hello.blob[0], 1, "the biometric blob, versioned");
    assert_eq!(hello.sealed_key[0], 1, "the sealed key, versioned");
    client.lock();

    let again = client
        .unlock_windows_hello(
            Signer::WindowsHello,
            &hello.blob,
            &hello.sealed_key,
            challenge.expose_secret(),
            &signature(1),
            &device,
            tokens(),
        )
        .unwrap();
    assert!(client.is_unlocked());
    assert_eq!(client.items(&vault).unwrap().len(), 1);
    // A fresh blob and sealed key, under the same signature, for next time.
    assert_ne!(again.blob, hello.blob);
    client.lock();
    client
        .unlock_windows_hello(
            Signer::WindowsHello,
            &again.blob,
            &again.sealed_key,
            challenge.expose_secret(),
            &signature(1),
            &device,
            tokens(),
        )
        .unwrap();
    assert!(client.is_unlocked());
}

#[test]
fn windows_hello_refuses_another_signature_challenge_or_device() {
    let (mut client, _, _) = unlocked();
    let device = new_id().unwrap();
    let challenge = pwm_client::windows_hello_challenge().unwrap();
    let hello = client
        .export_windows_hello(
            Signer::WindowsHello,
            &device,
            challenge.expose_secret(),
            &signature(1),
        )
        .unwrap();
    client.lock();
    let other = pwm_client::windows_hello_challenge().unwrap();
    for (sig, ch, dev, sealed) in [
        (
            signature(2),
            challenge.expose_secret(),
            device.clone(),
            hello.sealed_key.clone(),
        ),
        (
            signature(1),
            other.expose_secret(),
            device.clone(),
            hello.sealed_key.clone(),
        ),
        (
            signature(1),
            challenge.expose_secret(),
            new_id().unwrap(),
            hello.sealed_key.clone(),
        ),
        (
            signature(1),
            challenge.expose_secret(),
            device.clone(),
            vec![1, 2, 3],
        ),
    ] {
        assert_eq!(
            code(client.unlock_windows_hello(
                Signer::WindowsHello,
                &hello.blob,
                &sealed,
                ch,
                &sig,
                &dev,
                tokens()
            )),
            "biometric_unlock_failed"
        );
        assert!(!client.is_unlocked());
    }
}

// --- Linux (D71): the same, with the key holder's 32-byte answer ---

#[test]
fn linux_unlocks_with_the_key_holders_answer_and_not_as_windows() {
    let (mut client, _, vault) = unlocked();
    let device = new_id().unwrap();
    let challenge = pwm_client::windows_hello_challenge().unwrap();
    let answer = [0x42; 32];
    let linux = client
        .export_windows_hello(
            Signer::LinuxPolkit,
            &device,
            challenge.expose_secret(),
            &answer,
        )
        .unwrap();
    client.lock();
    // Taken as a Windows signature, the same bytes open nothing.
    assert_eq!(
        code(client.unlock_windows_hello(
            Signer::WindowsHello,
            &linux.blob,
            &linux.sealed_key,
            challenge.expose_secret(),
            &[0x42; 64],
            &device,
            tokens(),
        )),
        "biometric_unlock_failed"
    );
    client
        .unlock_windows_hello(
            Signer::LinuxPolkit,
            &linux.blob,
            &linux.sealed_key,
            challenge.expose_secret(),
            &answer,
            &device,
            tokens(),
        )
        .unwrap();
    assert_eq!(client.items(&vault).unwrap().len(), 1);
}
