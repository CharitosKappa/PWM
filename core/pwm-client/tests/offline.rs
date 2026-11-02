//! The client without a server (D31): input checks, unlock and lock, and
//! items as JSON. The server conversations are tested end to end in
//! `server/tests/client.rs`.

use pwm_client::{Client, ClientError, MemoryStore, Platform, Step, Store, Tokens, new_id};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, VaultRecord};
use pwm_vault::{Item, ItemId, ItemKind};
use serde_json::Value;

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";

/// A cache holding an account and one vault, as after a login; returns the
/// Secret Key text and the vault id.
fn signed_in_store() -> (MemoryStore, String, String) {
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
    (store, secret_key, ItemId::from_bytes(vault_id).to_string())
}

fn tokens() -> Tokens {
    Tokens::new("pwma_x".to_owned(), "pwmr_x".to_owned())
}

fn note_json(id: &str, title: &str) -> String {
    let id: ItemId = id.parse().unwrap();
    let item = Item::new(ItemKind::Note, title, T0);
    item.to_json_with_id(&id).unwrap().expose_secret().clone()
}

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> &'static str {
    result.unwrap_err().code()
}

#[test]
fn sign_up_checks_its_input_and_starts_the_conversation() {
    let mut client = Client::new(MemoryStore::new());
    client.set_signup_params(KdfParams::MOBILE_FLOOR).unwrap();
    let device = new_id().unwrap();
    assert_eq!(
        code(client.start_signup(
            "a@example.com",
            "password",
            &device,
            Platform::Linux,
            "Laptop"
        )),
        "weak_password"
    );
    assert_eq!(
        code(client.start_signup(
            "a@example.com",
            PASSWORD,
            "not-an-id",
            Platform::Linux,
            "Laptop"
        )),
        "invalid_id"
    );
    let Step::Send(request) = client
        .start_signup(
            "a@example.com",
            PASSWORD,
            &device,
            Platform::Linux,
            "Laptop",
        )
        .unwrap()
    else {
        panic!("expected a request");
    };
    assert_eq!(request.path, "/v1/auth/register/start");
    let body: Value = serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
    assert!(body["account_id"].is_string() && body["registration_request"].is_string());
    // One conversation at a time.
    assert_eq!(
        code(client.start_login("a@example.com", PASSWORD, "A1-x", &device, Platform::Linux)),
        "busy"
    );
    assert_eq!(code(client.network_failed()), "network");
    assert_eq!(
        code(client.start_login(
            "a@example.com",
            PASSWORD,
            "not a key",
            &device,
            Platform::Linux
        )),
        "invalid_secret_key"
    );
}

#[test]
fn unlock_lock_and_items_as_json() {
    let (store, secret_key, vault) = signed_in_store();
    let mut client = Client::new(store);
    assert!(!client.is_unlocked());
    assert_eq!(code(client.vault_ids()), "locked");
    assert_eq!(
        code(client.unlock("wrong password", &secret_key, tokens())),
        "wrong_password"
    );
    assert_eq!(
        code(client.unlock(PASSWORD, "A1-nope", tokens())),
        "invalid_secret_key"
    );
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    assert_eq!(client.vault_ids().unwrap(), std::slice::from_ref(&vault));

    // New, then edited: the edit keeps the old version in history.
    let id = new_id().unwrap();
    assert_eq!(
        client
            .save_item(&vault, &note_json(&id, "first"), T0)
            .unwrap(),
        id
    );
    client
        .save_item(&vault, &note_json(&id, "second"), T0 + 1)
        .unwrap();
    let json = client.item(&vault, &id).unwrap().unwrap();
    let item: Value = serde_json::from_str(json.expose_secret()).unwrap();
    assert_eq!(item["id"], id.as_str());
    assert_eq!(item["title"], "second");
    assert_eq!(item["history"][0]["title"], "first");
    assert_eq!(client.items(&vault).unwrap().len(), 1);

    // Trash and restore; then delete.
    client.set_trashed(&vault, &id, true, T0 + 2).unwrap();
    let trashed: Value =
        serde_json::from_str(client.item(&vault, &id).unwrap().unwrap().expose_secret()).unwrap();
    assert!(trashed["trashed_at"].is_i64());
    client.set_trashed(&vault, &id, false, T0 + 3).unwrap();
    client.delete_item(&vault, &id).unwrap();
    assert!(client.item(&vault, &id).unwrap().is_none());

    // Bad input.
    assert_eq!(code(client.save_item(&vault, "{}", T0)), "invalid_item");
    assert_eq!(
        code(client.save_item("nope", &note_json(&id, "x"), T0)),
        "invalid_id"
    );
    let unknown = new_id().unwrap();
    assert_eq!(
        code(client.save_item(&unknown, &note_json(&new_id().unwrap(), "x"), T0)),
        "unknown_vault"
    );
    assert_eq!(
        code(client.set_trashed(&vault, &new_id().unwrap(), true, T0)),
        "unknown_item"
    );
    assert_eq!(
        code(client.receive(
            pwm_client::HttpResponse::new(200, Vec::new(), Vec::new()),
            T0
        )),
        "not_running"
    );

    client.lock();
    assert!(!client.is_unlocked());
    assert_eq!(code(client.items(&vault)), "locked");
    assert!(client.account_id().unwrap().is_some());
    // The cache still holds only ciphertext and ids: the snapshot shows no title.
    let snapshot = client.store().snapshot();
    assert!(!snapshot.windows(6).any(|w| w == b"second"));
}

#[test]
fn error_codes_are_stable() {
    for (error, code) in [
        (ClientError::Locked, "locked"),
        (ClientError::WrongPassword, "wrong_password"),
        (ClientError::InvalidCredentials, "invalid_credentials"),
        (ClientError::ItemTooLarge, "item_too_large"),
        (ClientError::RateLimited(5), "rate_limited"),
        (ClientError::Server(500), "server"),
    ] {
        assert_eq!(error.code(), code);
        assert!(!error.to_string().is_empty());
    }
}
