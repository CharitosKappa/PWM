//! Client-side flows for API tests (D26): registration and login through
//! the core's OPAQUE client, as an app would do them.

use axum::Router;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::{
    AccountKeyset, KdfParams, OpaqueLogin, OpaqueRegistration, Secret, ServerSecrets, SymmetricKey,
    generate_id,
};
use pwm_server::{AppState, app};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{TestDb, call, migrated_db};

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(v: &Value) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(v.as_str().unwrap()).unwrap()
}

pub fn new_id() -> Uuid {
    Uuid::from_bytes(generate_id().unwrap())
}

pub async fn setup() -> Option<(TestDb, Router)> {
    let db = migrated_db().await?;
    let router = app(AppState::new(
        db.pool.clone(),
        ServerSecrets::generate().unwrap(),
    ));
    Some((db, router))
}

pub struct Account {
    pub id: Uuid,
    pub auth_secret: Secret<[u8; 32]>,
    pub keyset: AccountKeyset,
    pub sealed_keyset: Vec<u8>,
    pub salt: [u8; 16],
    pub device: Uuid,
    pub tokens: Value,
}

impl Account {
    pub fn access(&self) -> &str {
        self.tokens["access_token"].as_str().unwrap()
    }

    pub fn refresh(&self) -> &str {
        self.tokens["refresh_token"].as_str().unwrap()
    }
}

pub fn device_json(account: &Uuid, keyset: &AccountKeyset, device: &Uuid, name: &str) -> Value {
    let sealed = keyset
        .seal_device_name(account.as_bytes(), device.as_bytes(), name)
        .unwrap();
    json!({ "id": device, "platform": "macos", "name_sealed": b64(&sealed.to_bytes()) })
}

/// The registration body for a new account; returns it with the account.
pub async fn registration(app: &Router, email: &str) -> (Value, Account) {
    let id = new_id();
    let auth_secret = Secret::new(generate_id().unwrap().repeat(2).try_into().unwrap());
    let keyset = AccountKeyset::generate().unwrap();
    let sealed_keyset = keyset
        .seal(&SymmetricKey::generate().unwrap(), id.as_bytes())
        .unwrap()
        .to_bytes();
    let salt = generate_id().unwrap();
    let device = new_id();

    let (client, request) = OpaqueRegistration::start(&auth_secret).unwrap();
    let (status, body) = call(
        app,
        "POST",
        "/v1/auth/register/start",
        None,
        Some(json!({ "account_id": id, "registration_request": b64(&request) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let upload = client
        .finish(
            &auth_secret,
            id.as_bytes(),
            &unb64(&body["registration_response"]),
        )
        .unwrap();
    let d = KdfParams::DEFAULT;
    let request = json!({
        "email": email, "account_id": id, "registration_upload": b64(&upload), "salt": b64(&salt),
        "kdf": { "memory_kib": d.memory_kib, "iterations": d.iterations, "parallelism": d.parallelism },
        "sealed_keyset": b64(&sealed_keyset), "x25519_pub": b64(&keyset.x25519_public()),
        "ed25519_pub": b64(&keyset.ed25519_public()),
        "device": device_json(&id, &keyset, &device, "Laptop"),
    });
    let account = Account {
        id,
        auth_secret,
        keyset,
        sealed_keyset,
        salt,
        device,
        tokens: Value::Null,
    };
    (request, account)
}

pub async fn register(app: &Router, email: &str) -> Account {
    let (request, mut account) = registration(app, email).await;
    let (status, tokens) = call(app, "POST", "/v1/auth/register/finish", None, Some(request)).await;
    assert_eq!(status, StatusCode::CREATED, "{tokens}");
    account.tokens = tokens;
    account
}

/// Starts a login; returns the client state and the login id.
pub async fn login_start(app: &Router, account: &Account) -> (OpaqueLogin, Value, Vec<u8>) {
    let (client, request) = OpaqueLogin::start(&account.auth_secret).unwrap();
    let (status, body) = call(
        app,
        "POST",
        "/v1/auth/login/start",
        None,
        Some(json!({ "account_id": account.id, "credential_request": b64(&request) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let response = unb64(&body["credential_response"]);
    (client, body["login_id"].clone(), response)
}

pub async fn login(app: &Router, account: &Account, device: &Uuid) -> (StatusCode, Value) {
    let (client, login_id, response) = login_start(app, account).await;
    let finalization = client
        .finish(&account.auth_secret, account.id.as_bytes(), &response)
        .unwrap();
    call(
        app,
        "POST",
        "/v1/auth/login/finish",
        None,
        Some(json!({
            "login_id": login_id, "credential_finalization": b64(&finalization),
            "device": device_json(&account.id, &account.keyset, device, "Phone"),
        })),
    )
    .await
}

pub async fn refresh(app: &Router, token: &str) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        "/v1/auth/refresh",
        None,
        Some(json!({ "refresh_token": token })),
    )
    .await
}

pub async fn get_account(app: &Router, access: &str) -> StatusCode {
    call(app, "GET", "/v1/account", Some(access), None).await.0
}

pub fn unauthorized() -> (StatusCode, Value) {
    (StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" }))
}

pub fn invalid_credentials() -> (StatusCode, Value) {
    (
        StatusCode::UNAUTHORIZED,
        json!({ "error": "invalid_credentials" }),
    )
}
