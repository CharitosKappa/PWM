//! The client's locked sync (D50 §1), with scripted answers: it needs only
//! the tokens, keeps the vault locked, never loses refreshed tokens, and
//! hands over cleanly when the user unlocks or locks during a run. The
//! engine's rules are tested in `pwm-sync`; the whole path against the real
//! server in `server/tests/client.rs`.

use pwm_client::{
    Client, ClientError, HttpRequest, HttpResponse, MemoryStore, Outcome, Step, Store, SyncEvent,
    Tokens,
};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, VaultRecord};
use pwm_vault::ItemId;
use serde_json::json;

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";

/// A cache holding an account and one vault; returns the Secret Key text.
fn signed_in_store() -> (MemoryStore, String) {
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
    (store, new.secret_key.to_text().expose_secret().clone())
}

fn tokens() -> Tokens {
    Tokens::new("pwma_old".to_owned(), "pwmr_old".to_owned())
}

fn sent(step: Step) -> HttpRequest {
    match step {
        Step::Send(request) => request,
        other => panic!("expected a request, got {other:?}"),
    }
}

fn path(request: &HttpRequest) -> String {
    format!("{} {}", request.method.as_str(), request.path)
}

fn answer(status: u16, body: serde_json::Value) -> HttpResponse {
    HttpResponse::new(status, Vec::new(), body.to_string().into_bytes())
}

/// The server's vault list, now empty: the cached vault goes.
fn no_vaults() -> HttpResponse {
    answer(200, json!([]))
}

fn new_tokens() -> HttpResponse {
    answer(
        200,
        json!({ "access_token": "pwma_new", "refresh_token": "pwmr_new" }),
    )
}

fn refreshed(client: &mut Client<MemoryStore>) -> bool {
    client
        .take_events()
        .iter()
        .any(|e| matches!(e, SyncEvent::TokensRefreshed(t) if t.refresh() == "pwmr_new"))
}

#[test]
fn it_needs_an_account() {
    let mut client = Client::new(MemoryStore::new());
    assert_eq!(
        client.start_locked_sync(tokens()).unwrap_err().code(),
        "locked"
    );
}

#[test]
fn it_syncs_with_the_tokens_and_stays_locked() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    let request = sent(client.start_locked_sync(tokens()).unwrap());
    assert_eq!(path(&request), "GET /v1/vaults");
    assert!(
        request
            .headers
            .iter()
            .any(|(n, v)| *n == "authorization" && *v == "Bearer pwma_old")
    );
    // Busy while it runs; items stay out of reach.
    assert_eq!(client.start_sync().unwrap_err().code(), "busy");
    assert!(!client.is_unlocked());

    let step = client.receive(no_vaults(), T0).unwrap();
    assert!(matches!(step, Step::Done(Outcome::Synced(_))));
    assert!(!client.is_unlocked());
    assert!(client.store().vaults().unwrap().is_empty());
    assert!(matches!(client.vault_ids(), Err(ClientError::Locked)));
    // Idle again.
    assert!(matches!(
        client.start_locked_sync(tokens()).unwrap(),
        Step::Send(_)
    ));
}

#[test]
fn refreshed_tokens_are_kept() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    client.start_locked_sync(tokens()).unwrap();
    let refresh = sent(client.receive(answer(401, json!({})), T0).unwrap());
    assert_eq!(path(&refresh), "POST /v1/auth/refresh");
    let again = sent(client.receive(new_tokens(), T0).unwrap());
    assert!(
        again
            .headers
            .iter()
            .any(|(n, v)| *n == "authorization" && *v == "Bearer pwma_new")
    );
    client.receive(no_vaults(), T0).unwrap();
    assert!(refreshed(&mut client));
}

#[test]
fn a_refused_refresh_token_signs_out_and_keeps_the_cache() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    client.start_locked_sync(tokens()).unwrap();
    client.receive(answer(401, json!({})), T0).unwrap();
    let step = client.receive(answer(401, json!({})), T0).unwrap();
    assert!(matches!(step, Step::Done(Outcome::SignedOut)));
    assert!(client.account_id().unwrap().is_some());
    assert_eq!(client.store().vaults().unwrap().len(), 1);
}

#[test]
fn unlocking_during_a_locked_sync_carries_it_on() {
    let (store, secret_key) = signed_in_store();
    let mut client = Client::new(store);
    client.start_locked_sync(tokens()).unwrap();
    client.receive(answer(401, json!({})), T0).unwrap();

    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    assert!(client.is_unlocked());
    // The run goes on with the refreshed tokens, now unlocked.
    let again = sent(client.receive(new_tokens(), T0).unwrap());
    assert!(
        again
            .headers
            .iter()
            .any(|(n, v)| *n == "authorization" && *v == "Bearer pwma_new")
    );
    let step = client.receive(no_vaults(), T0).unwrap();
    assert!(matches!(step, Step::Done(Outcome::Synced(_))));
    assert!(client.is_unlocked());
    assert!(client.vault_ids().unwrap().is_empty());
    assert!(refreshed(&mut client));
}

#[test]
fn locking_during_a_sync_carries_it_on_without_keys() {
    let (store, secret_key) = signed_in_store();
    let mut client = Client::new(store);
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    client.start_sync().unwrap();
    client.receive(answer(401, json!({})), T0).unwrap();

    client.lock();
    assert!(!client.is_unlocked());
    assert!(matches!(client.vault_ids(), Err(ClientError::Locked)));
    client.receive(new_tokens(), T0).unwrap();
    let step = client.receive(no_vaults(), T0).unwrap();
    assert!(matches!(step, Step::Done(Outcome::Synced(_))));
    assert!(!client.is_unlocked());
    assert!(refreshed(&mut client));
}

#[test]
fn signing_out_stops_a_locked_sync() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    client.start_locked_sync(tokens()).unwrap();
    client.sign_out().unwrap();
    assert!(client.account_id().unwrap().is_none());
    assert_eq!(
        client.receive(no_vaults(), T0).unwrap_err().code(),
        "not_running"
    );
}

#[test]
fn unlocked_it_is_an_ordinary_sync() {
    let (store, secret_key) = signed_in_store();
    let mut client = Client::new(store);
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    client.start_locked_sync(tokens()).unwrap();
    let step = client.receive(no_vaults(), T0).unwrap();
    assert!(matches!(step, Step::Done(Outcome::Synced(_))));
    assert!(client.is_unlocked());
}
