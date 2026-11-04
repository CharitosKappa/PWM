//! Touch ID unlock for the browser extension through the client (decision
//! D48 §5): the blob the extension keeps, the key the Mac's helper keeps
//! behind Touch ID, and the tokens sealed on the extension's disk.

use pwm_client::{Client, ClientError, MemoryStore, Tokens, new_id};
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
fn the_extension_unlocks_without_the_master_password() {
    let (mut client, _, vault) = unlocked();
    let device = new_id().unwrap();
    let unlock = client.export_browser_unlock(&device).unwrap();
    assert_eq!(unlock.blob[0], 1, "versioned");
    let sealed = client.seal_tokens("pwma_x", "pwmr_x").unwrap();
    client.lock();
    assert_eq!(code(client.items(&vault)), "locked");

    client
        .unlock_browser(
            &unlock.blob,
            unlock.key.expose_secret(),
            &device,
            Some(&sealed),
        )
        .unwrap();
    assert!(client.is_unlocked());
    assert_eq!(client.items(&vault).unwrap().len(), 1);
    assert!(!format!("{unlock:?}").contains(&format!("{:?}", unlock.key.expose_secret())));
}

#[test]
fn only_its_own_key_device_and_account_open_it() {
    let (mut client, _, _) = unlocked();
    let device = new_id().unwrap();
    let unlock = client.export_browser_unlock(&device).unwrap();
    let face_id = client.export_biometric_unlock(&device).unwrap();
    let (other_client, _, _) = unlocked();
    let other = other_client.export_browser_unlock(&device).unwrap();
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
        // The Face ID blob is not a browser blob.
        (
            &face_id.blob[..],
            &face_id.key.expose_secret()[..],
            device.as_str(),
        ),
    ] {
        assert_eq!(
            code(client.unlock_browser(blob, key, device, None)),
            "browser_unlock_failed"
        );
        assert!(!client.is_unlocked());
    }
    assert_eq!(
        code(client.unlock_browser(&unlock.blob, &wrong_key, "not-an-id", None)),
        "invalid_id"
    );
}

#[test]
fn it_needs_an_unlocked_vault_to_make() {
    let (mut client, secret_key, _) = unlocked();
    client.lock();
    assert_eq!(
        code(client.export_browser_unlock(&new_id().unwrap())),
        "locked"
    );
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    assert!(client.export_browser_unlock(&new_id().unwrap()).is_ok());
}
