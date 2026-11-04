//! Ending the session on the server before signing out (D61), with scripted
//! answers: it works locked or unlocked, wins over a running sync, ends with
//! `LoggedOut` whenever the session is over, and never clears anything
//! itself. The whole path against the real server is in
//! `server/tests/client.rs`.

use pwm_client::{
    Client, HttpRequest, HttpResponse, MemoryStore, Outcome, Step, Store, SyncEvent, Tokens,
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

fn bearer(request: &HttpRequest) -> Option<&str> {
    request
        .headers
        .iter()
        .find(|(n, _)| *n == "authorization")
        .map(|(_, v)| v.as_str())
}

fn answer(status: u16, body: serde_json::Value) -> HttpResponse {
    HttpResponse::new(status, Vec::new(), body.to_string().into_bytes())
}

fn no_content() -> HttpResponse {
    HttpResponse::new(204, Vec::new(), Vec::new())
}

fn logged_out(step: Step) -> bool {
    matches!(step, Step::Done(Outcome::LoggedOut))
}

#[test]
fn locked_it_sends_the_stored_tokens_and_keeps_the_cache() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    let request = sent(client.start_sign_out(Some(tokens())).unwrap());
    assert_eq!(path(&request), "POST /v1/auth/logout");
    assert_eq!(bearer(&request), Some("Bearer pwma_old"));
    assert!(request.body.is_none());
    assert!(logged_out(client.receive(no_content(), T0).unwrap()));
    assert!(!client.is_unlocked());
    // Wiping is the app's next call, not this one's.
    assert!(client.store().account().unwrap().is_some());
    assert_eq!(client.store().vaults().unwrap().len(), 1);
    client.sign_out().unwrap();
    assert!(client.store().account().unwrap().is_none());
}

#[test]
fn unlocked_it_uses_the_open_session() {
    let (store, secret_key) = signed_in_store();
    let mut client = Client::new(store);
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    // The extension has no tokens to pass; the engine has them.
    let request = sent(client.start_sign_out(None).unwrap());
    assert_eq!(path(&request), "POST /v1/auth/logout");
    assert_eq!(bearer(&request), Some("Bearer pwma_old"));
    assert!(logged_out(client.receive(no_content(), T0).unwrap()));
    assert!(client.is_unlocked());
}

#[test]
fn locked_without_tokens_or_without_an_account_it_sends_nothing() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    assert_eq!(client.start_sign_out(None).unwrap_err().code(), "locked");
    // A revoked device's cache is already gone.
    let mut empty = Client::new(MemoryStore::new());
    assert_eq!(
        empty.start_sign_out(Some(tokens())).unwrap_err().code(),
        "locked"
    );
}

#[test]
fn an_expired_access_token_is_refreshed_first() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    client.start_sign_out(Some(tokens())).unwrap();
    let refresh = sent(client.receive(answer(401, json!({})), T0).unwrap());
    assert_eq!(path(&refresh), "POST /v1/auth/refresh");
    let again = sent(
        client
            .receive(
                answer(
                    200,
                    json!({ "access_token": "pwma_new", "refresh_token": "pwmr_new" }),
                ),
                T0,
            )
            .unwrap(),
    );
    assert_eq!(path(&again), "POST /v1/auth/logout");
    assert_eq!(bearer(&again), Some("Bearer pwma_new"));
    assert!(logged_out(client.receive(no_content(), T0).unwrap()));
    assert!(
        client
            .take_events()
            .iter()
            .any(|e| matches!(e, SyncEvent::TokensRefreshed(_)))
    );
}

#[test]
fn signing_out_drops_tokens_not_yet_taken() {
    // The app gave up waiting after the refresh; the wipe must not leave
    // the new tokens for it to store afterwards.
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    client.start_sign_out(Some(tokens())).unwrap();
    client.receive(answer(401, json!({})), T0).unwrap();
    sent(
        client
            .receive(
                answer(
                    200,
                    json!({ "access_token": "pwma_new", "refresh_token": "pwmr_new" }),
                ),
                T0,
            )
            .unwrap(),
    );
    client.sign_out().unwrap();
    assert!(client.take_events().is_empty());
    assert_eq!(
        client.receive(no_content(), T0).unwrap_err().code(),
        "not_running"
    );
}

#[test]
fn a_session_already_over_counts_as_logged_out() {
    // The refresh token is refused: nothing left to end.
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    client.start_sign_out(Some(tokens())).unwrap();
    client.receive(answer(401, json!({})), T0).unwrap();
    let step = client
        .receive(answer(401, json!({ "error": "unauthorized" })), T0)
        .unwrap();
    assert!(logged_out(step));

    // A revoked device: the engine clears the cache, as on any request.
    let (store, secret_key) = signed_in_store();
    let mut client = Client::new(store);
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    client.start_sign_out(None).unwrap();
    let step = client
        .receive(answer(403, json!({ "error": "device_revoked" })), T0)
        .unwrap();
    assert!(logged_out(step));
    assert!(!client.is_unlocked());
    assert!(client.store().account().unwrap().is_none());
}

#[test]
fn a_running_sync_is_abandoned() {
    let (store, secret_key) = signed_in_store();
    let mut client = Client::new(store);
    let sync = sent(client.start_locked_sync(tokens()).unwrap());
    assert_eq!(path(&sync), "GET /v1/vaults");
    let request = sent(client.start_sign_out(Some(tokens())).unwrap());
    assert_eq!(path(&request), "POST /v1/auth/logout");
    assert!(logged_out(client.receive(no_content(), T0).unwrap()));

    // Unlocked too.
    client.unlock(PASSWORD, &secret_key, tokens()).unwrap();
    sent(client.start_sync().unwrap());
    let request = sent(client.start_sign_out(None).unwrap());
    assert_eq!(path(&request), "POST /v1/auth/logout");
    assert!(logged_out(client.receive(no_content(), T0).unwrap()));
    assert!(client.is_unlocked());
}

#[test]
fn failures_end_it_and_leave_the_client_idle() {
    let (store, _) = signed_in_store();
    let mut client = Client::new(store);
    client.start_sign_out(Some(tokens())).unwrap();
    assert!(matches!(
        client.receive(answer(503, json!({})), T0).unwrap(),
        Step::Wait { .. }
    ));
    client.start_sign_out(Some(tokens())).unwrap();
    assert!(matches!(
        client.network_failed().unwrap(),
        Step::Wait { .. }
    ));
    client.start_sign_out(Some(tokens())).unwrap();
    assert_eq!(
        client
            .receive(answer(400, json!({ "error": "invalid_request" })), T0)
            .unwrap_err()
            .code(),
        "server"
    );
    // Idle again, and nothing was cleared.
    assert!(matches!(
        client.start_locked_sync(tokens()).unwrap(),
        Step::Send(_)
    ));
    assert!(client.store().account().unwrap().is_some());
}
