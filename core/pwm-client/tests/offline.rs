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

fn login_json(id: &str, title: &str, username: &str, url: &str) -> String {
    let mut login = pwm_vault::Login::default();
    login.username = username.to_owned();
    let mut login_url = pwm_vault::LoginUrl::default();
    login_url.url = url.to_owned();
    login.urls = vec![login_url];
    let item = Item::new(ItemKind::Login(login), title, T0);
    item.to_json_with_id(&id.parse().unwrap())
        .unwrap()
        .expose_secret()
        .clone()
}

#[test]
fn autofill_through_the_client() {
    let (store, secret_key, vault) = signed_in_store();
    let mut client = Client::new(store);
    assert_eq!(
        code(client.autofill_matches("https://example.com", None)),
        "locked"
    );
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    let bank = new_id().unwrap();
    client
        .save_item(
            &vault,
            &login_json(&bank, "Bank", "alice", "bank.example"),
            T0,
        )
        .unwrap();
    let mail = new_id().unwrap();
    client
        .save_item(&vault, &login_json(&mail, "Mail", "al", "mail.other"), T0)
        .unwrap();
    client
        .save_item(&vault, &note_json(&new_id().unwrap(), "a note"), T0)
        .unwrap();

    let found = client
        .autofill_matches("https://login.bank.example/signin", None)
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].item_id.as_str(), found[0].username.as_str()),
        (bank.as_str(), "alice")
    );
    assert_eq!((found[0].matched, found[0].decision), ("domain", "fill"));
    let insecure = client
        .autofill_matches("http://bank.example/", None)
        .unwrap();
    assert_eq!(insecure[0].decision, "warn_insecure");
    let framed = client
        .autofill_matches("https://bank.example/", Some("https://evil.example/f"))
        .unwrap();
    assert_eq!(framed[0].decision, "refuse_cross_site");
    assert_eq!(
        code(client.autofill_matches("not a url", None)),
        "invalid_url"
    );
    // Trashed logins are not offered.
    client.set_trashed(&vault, &bank, true, T0 + 1).unwrap();
    assert!(
        client
            .autofill_matches("https://bank.example/", None)
            .unwrap()
            .is_empty()
    );

    let classes = client
        .classify_fields(
            r#"[{"type":"text","name":"user","visible":true},{"type":"password","visible":true}]"#,
            "https://bank.example/",
        )
        .unwrap();
    let classes: Value = serde_json::from_str(&classes).unwrap();
    assert_eq!(classes[0]["class"], "username");
    assert_eq!(classes[1]["class"], "current-password");
    assert_eq!(
        code(client.classify_fields("nope", "https://bank.example/")),
        "invalid_item"
    );

    client
        .autofill_record_step("tab", "https://bank.example/a", &bank, "username", T0)
        .unwrap();
    assert_eq!(
        client.autofill_next_step("tab", "https://bank.example/b", T0 + 1),
        Some((bank.clone(), "password"))
    );
    client.autofill_forget_tab("tab");
    assert_eq!(
        client.autofill_next_step("tab", "https://bank.example/b", T0 + 2),
        None
    );
    assert_eq!(
        code(client.autofill_record_step("tab", "https://bank.example/", &bank, "pin", T0)),
        "invalid_item"
    );

    // No release key exists yet, so every downloaded file is refused.
    assert_eq!(
        code(client.set_autofill_rules(b"anything", T0, 0)),
        "invalid_rules"
    );
}

#[test]
fn the_session_survives_a_service_worker_restart() {
    let (store, secret_key, vault) = signed_in_store();
    let mut client = Client::new(store);
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    let id = new_id().unwrap();
    client
        .save_item(&vault, &note_json(&id, "kept"), T0)
        .unwrap();
    let sealed = client.seal_tokens("pwma_a", "pwmr_r").unwrap();
    let session = client.export_session().unwrap();
    let snapshot = client.store().snapshot();
    // The sealed tokens never show the tokens.
    assert!(!sealed.windows(6).any(|w| w == b"pwmr_r"));

    // A new worker: the cache from disk, the session from memory.
    let mut restored = Client::new(MemoryStore::load(&snapshot).unwrap());
    assert_eq!(
        code(restored.restore_session(&session.blob, &[0u8; 32], Some(&sealed))),
        "wrong_password"
    );
    restored
        .restore_session(&session.blob, session.key.expose_secret(), Some(&sealed))
        .unwrap();
    assert!(restored.is_unlocked());
    assert_eq!(restored.items(&vault).unwrap().len(), 1);

    // After a browser restart: the password again, and the sealed tokens.
    let mut cold = Client::new(MemoryStore::load(&snapshot).unwrap());
    assert_eq!(
        code(cold.unlock_with_sealed_tokens("wrong password 123", &secret_key, Some(&sealed))),
        "wrong_password"
    );
    cold.unlock_with_sealed_tokens(PASSWORD, &secret_key, Some(&sealed))
        .unwrap();
    assert!(cold.is_unlocked());
    assert_eq!(
        code(cold.unlock_with_sealed_tokens(PASSWORD, &secret_key, Some(b"\x01junk"))),
        "storage"
    );

    // Another account's session is refused.
    let (other_store, other_key, _) = signed_in_store();
    let mut other = Client::new(other_store);
    other.unlock(PASSWORD, &other_key, tokens()).unwrap();
    let foreign = other.export_session().unwrap();
    let mut target = Client::new(MemoryStore::load(&snapshot).unwrap());
    assert_eq!(
        code(target.restore_session(&foreign.blob, foreign.key.expose_secret(), None)),
        "wrong_password"
    );
}

#[test]
fn the_current_code_of_a_login() {
    let (store, secret_key, vault) = signed_in_store();
    let mut client = Client::new(store);
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    let mut login = pwm_vault::Login::default();
    login.totp = Some("otpauth://totp/x?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_owned());
    let id = ItemId::generate().unwrap();
    let json = Item::new(ItemKind::Login(login), "2FA", T0)
        .to_json_with_id(&id)
        .unwrap();
    client.save_item(&vault, json.expose_secret(), T0).unwrap();
    // RFC 6238 test vector: SHA-1 at 59 s gives 94287082 (6 digits: 287082).
    let (code_value, left) = client
        .current_code(&vault, &id.to_string(), 59_000)
        .unwrap()
        .unwrap();
    assert_eq!(code_value.expose_secret(), "287082");
    assert_eq!(left, 1);
    let note = new_id().unwrap();
    client
        .save_item(&vault, &note_json(&note, "n"), T0)
        .unwrap();
    assert!(client.current_code(&vault, &note, T0).unwrap().is_none());
}
