//! Autofill for Android apps through the client (D58 §4): which logins are
//! linked to an app, and making a link.

use pwm_client::{Client, ClientError, MemoryStore, Store, Tokens, app_link};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, VaultRecord};
use pwm_vault::ItemId;
use serde_json::json;

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";
const BANK: &str = "com.example.bank";
const CERT: &str = "f0fd6c5b410f25cb25c3b53346c8972fae30f8ee7411df910480ad6b2d60db83";
const OTHER: &str = "a78b62a5165b4494b2fead9e76a280d22d937fee6251aece599446b2ea319b04";

/// An unlocked client with one empty vault; returns it and the vault id.
fn unlocked() -> (Client<MemoryStore>, String) {
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
    client
        .unlock(
            PASSWORD,
            &secret_key,
            Tokens::new("pwma_x".to_owned(), "pwmr_x".to_owned()),
        )
        .unwrap();
    (client, ItemId::from_bytes(vault_id).to_string())
}

fn login(
    client: &mut Client<MemoryStore>,
    vault: &str,
    title: &str,
    urls: &[&str],
    at: i64,
) -> String {
    let id = ItemId::generate().unwrap().to_string();
    let urls: Vec<_> = urls
        .iter()
        .map(|u| json!({"url": u, "match": "exact"}))
        .collect();
    let item = json!({
        "id": id, "v": 1, "type": "login", "title": title, "created_at": at, "updated_at": at,
        "login": {"username": format!("{title}-user"), "password": "pw", "urls": urls}
    });
    client.save_item(vault, &item.to_string(), at).unwrap()
}

#[test]
fn only_logins_linked_to_the_app_and_its_certificate_are_offered() {
    let (mut client, vault) = unlocked();
    let link = app_link(BANK, CERT).unwrap();
    let linked = login(
        &mut client,
        &vault,
        "Bank",
        &[&link, "https://bank.example"],
        T0,
    );
    login(
        &mut client,
        &vault,
        "Web only",
        &["https://bank.example"],
        T0,
    );
    login(
        &mut client,
        &vault,
        "Other signer",
        &[&app_link(BANK, OTHER).unwrap()],
        T0,
    );
    let imported = login(
        &mut client,
        &vault,
        "Imported",
        &[&format!("androidapp://{BANK}")],
        T0 + 1,
    );

    let found = client
        .autofill_app_matches(BANK, &[CERT.to_owned()])
        .unwrap();
    let titles: Vec<_> = found
        .iter()
        .map(|m| (m.title.as_str(), m.matched))
        .collect();
    assert_eq!(
        titles,
        vec![("Bank", "app"), ("Imported", "app_unverified")],
        "linked first, then unverified"
    );
    assert_eq!(found[0].item_id, linked);
    assert_eq!(found[0].decision, "fill");
    assert_eq!(found[1].item_id, imported);
    assert_eq!(
        found[1].decision, "confirm",
        "only from search, never suggested"
    );
    assert_eq!(found[0].username, "Bank-user");

    assert!(
        client
            .autofill_app_matches("com.example.other", &[CERT.to_owned()])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn trashed_logins_and_a_locked_vault() {
    let (mut client, vault) = unlocked();
    let id = login(
        &mut client,
        &vault,
        "Bank",
        &[&app_link(BANK, CERT).unwrap()],
        T0,
    );
    client.set_trashed(&vault, &id, true, T0 + 1).unwrap();
    assert!(
        client
            .autofill_app_matches(BANK, &[CERT.to_owned()])
            .unwrap()
            .is_empty()
    );
    client.lock();
    assert!(matches!(
        client.autofill_app_matches(BANK, &[CERT.to_owned()]),
        Err(ClientError::Locked)
    ));
}
