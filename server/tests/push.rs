//! Push over APNs (D50 §3) and FCM (D60), against fakes: who gets a
//! push, what it says, at most one a minute, the approval notice, FCM's
//! signed assertion, and tokens a provider refuses or a device no longer needs.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::flows::*;
use common::{TestDb, call, migrated_db, send};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::{EncodePrivateKey, LineEnding};
use pwm_crypto::{ServerSecrets, SymmetricKey};
use pwm_server::{
    APPROVAL_NOTICE, AppState, BACKGROUND_PAYLOAD, FCM_APPROVAL_DATA, FCM_SCOPE, FCM_SYNC_DATA,
    FcmConfig, Limits, Push, PushConfig, app, system_clock,
};
use serde_json::{Value, json};
use uuid::Uuid;

const PHONE_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LAPTOP_TOKEN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Got {
    token: String,
    headers: HeaderMap,
    body: String,
}

/// Records each push; answers 410 for tokens in `gone`.
#[derive(Clone, Default)]
struct Apns {
    got: Arc<Mutex<Vec<Got>>>,
    gone: Arc<Mutex<Vec<String>>>,
}

impl Apns {
    async fn start() -> (Self, String) {
        let apns = Self::default();
        let recorder = apns.clone();
        let router = Router::new().route(
            "/3/device/{token}",
            post(
                move |Path(token): Path<String>, headers: HeaderMap, body: String| {
                    let recorder = recorder.clone();
                    async move {
                        let gone = recorder.gone.lock().unwrap().contains(&token);
                        recorder.got.lock().unwrap().push(Got {
                            token,
                            headers,
                            body,
                        });
                        if gone {
                            (StatusCode::GONE, r#"{"reason":"Unregistered"}"#)
                        } else {
                            (StatusCode::OK, "")
                        }
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await });
        (apns, url)
    }

    /// Waits until `n` pushes arrived, then a little longer for extras.
    async fn wait_for(&self, n: usize) -> Vec<(String, HeaderMap, String)> {
        for _ in 0..100 {
            if self.got.lock().unwrap().len() >= n {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.got
            .lock()
            .unwrap()
            .iter()
            .map(|g| (g.token.clone(), g.headers.clone(), g.body.clone()))
            .collect()
    }
}

fn apns_key() -> SigningKey {
    SigningKey::from_slice(&[7u8; 32]).unwrap()
}

async fn setup() -> Option<(TestDb, Router, Apns)> {
    let db = migrated_db().await?;
    let (apns, url) = Apns::start().await;
    let pem = apns_key().to_pkcs8_pem(LineEnding::LF).unwrap();
    let config = PushConfig::new(
        &pem,
        "KEY1234567",
        "TEAM123456",
        "app.pwm.ios",
        "app.pwm.mac",
        Some(&url),
    )
    .unwrap();
    let state = AppState::new(db.pool.clone(), ServerSecrets::generate().unwrap())
        .with_limits(Limits::unlimited(), system_clock())
        .with_push(Push::start(db.pool.clone(), Some(config), None));
    Some((db, app(state), apns))
}

async fn set_push(app: &Router, access: &str, app_name: &str, token: &str) -> StatusCode {
    call(
        app,
        "PUT",
        "/v1/devices/self/push",
        Some(access),
        Some(json!({ "app": app_name, "token": token, "environment": "sandbox" })),
    )
    .await
    .0
}

/// The account's first device ("laptop") with a vault, and a second one
/// ("phone"); returns the account, the phone's access token and the vault.
async fn two_devices(app: &Router) -> (Account, String, Uuid, SymmetricKey) {
    let account = register(app, "push@example.com").await;
    let (status, phone) = login(app, &account, &new_id()).await;
    assert_eq!(status, StatusCode::OK);
    let vault = new_id();
    let key = SymmetricKey::generate().unwrap();
    let wrapped = account
        .keyset
        .wrap_vault_key(&key, vault.as_bytes())
        .unwrap();
    let (status, _) = call(
        app,
        "POST",
        "/v1/vaults",
        Some(account.access()),
        Some(json!({ "id": vault, "wrapped_key": b64(&wrapped.to_bytes()) })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let phone = phone["access_token"].as_str().unwrap().to_owned();
    (account, phone, vault, key)
}

async fn write_item(app: &Router, access: &str, vault: &Uuid, key: &SymmetricKey) -> Uuid {
    let item = new_id();
    let blob = pwm_crypto::seal(
        key,
        b"item",
        &pwm_crypto::Aad::item(1, vault.as_bytes(), item.as_bytes(), 1),
    )
    .unwrap();
    let reply = send(
        app,
        "PUT",
        &format!("/v1/vaults/{vault}/items/{item}"),
        Some(access),
        &[("if-none-match", "*")],
        Some(json!({ "revision": "1", "blob": b64(&blob.to_bytes()) })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED);
    item
}

async fn push_rows(db: &TestDb) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM push_tokens")
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_token_is_checked_registered_and_removed() {
    let Some((db, app, _)) = setup().await else {
        return;
    };
    let account = register(&app, "push@example.com").await;
    let access = account.access();
    for (app_name, token, environment) in [
        ("android", PHONE_TOKEN, "sandbox"),
        ("ios", "abc", "sandbox"),
        ("ios", &PHONE_TOKEN[..62], "sandbox"),
        ("ios", &"zz".repeat(32), "sandbox"),
        ("ios", PHONE_TOKEN, "staging"),
    ] {
        let (status, _) = call(
            &app,
            "PUT",
            "/v1/devices/self/push",
            Some(access),
            Some(json!({ "app": app_name, "token": token, "environment": environment })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{app_name} {token}");
    }
    let (status, _) = call(
        &app,
        "PUT",
        "/v1/devices/self/push",
        None,
        Some(json!({ "app": "ios", "token": PHONE_TOKEN, "environment": "sandbox" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    assert_eq!(
        set_push(&app, access, "ios", PHONE_TOKEN).await,
        StatusCode::NO_CONTENT
    );
    // Again, or with a new token: still one row.
    assert_eq!(
        set_push(&app, access, "ios", PHONE_TOKEN).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_push(&app, access, "ios", LAPTOP_TOKEN).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(push_rows(&db).await, 1);
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token FROM push_tokens")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(stored, vec![0xbb; 32], "kept as bytes");

    // A token another device held moves to the one registering it.
    let (_, other) = login(&app, &account, &new_id()).await;
    let other = other["access_token"].as_str().unwrap();
    assert_eq!(
        set_push(&app, other, "ios", LAPTOP_TOKEN).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(push_rows(&db).await, 1);

    let (status, _) = call(&app, "DELETE", "/v1/devices/self/push", Some(other), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(push_rows(&db).await, 0);
    db.drop().await;
}

#[tokio::test]
async fn a_change_pushes_to_the_other_devices_only_and_says_nothing() {
    let Some((db, app, apns)) = setup().await else {
        return;
    };
    let (account, phone, vault, key) = two_devices(&app).await;
    set_push(&app, &phone, "ios", PHONE_TOKEN).await;
    set_push(&app, account.access(), "macos", LAPTOP_TOKEN).await;

    write_item(&app, account.access(), &vault, &key).await;
    let got = apns.wait_for(1).await;
    assert_eq!(got.len(), 1, "the writer gets none");
    let (token, headers, body) = &got[0];
    assert_eq!(token, PHONE_TOKEN);
    assert_eq!(body, BACKGROUND_PAYLOAD);
    assert_eq!(headers["apns-topic"], "app.pwm.ios");
    assert_eq!(headers["apns-push-type"], "background");
    assert_eq!(headers["apns-priority"], "5");
    assert!(headers.contains_key("apns-expiration"));

    // The provider token is ES256, signed with the APNs key.
    let auth = headers["authorization"].to_str().unwrap();
    let jwt = auth.strip_prefix("bearer ").unwrap();
    let parts: Vec<&str> = jwt.split('.').collect();
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    assert_eq!(header, json!({ "alg": "ES256", "kid": "KEY1234567" }));
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(claims["iss"], "TEAM123456");
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
    let verifying: VerifyingKey = *apns_key().verifying_key();
    assert!(
        verifying
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
            .is_ok()
    );
    db.drop().await;
}

#[tokio::test]
async fn a_device_gets_at_most_one_push_a_minute() {
    let Some((db, app, apns)) = setup().await else {
        return;
    };
    let (account, phone, vault, key) = two_devices(&app).await;
    set_push(&app, &phone, "ios", PHONE_TOKEN).await;
    for _ in 0..3 {
        write_item(&app, account.access(), &vault, &key).await;
    }
    assert_eq!(apns.wait_for(1).await.len(), 1);
    db.drop().await;
}

#[tokio::test]
async fn an_approval_request_sends_the_fixed_notice_to_every_device() {
    let Some((db, app, apns)) = setup().await else {
        return;
    };
    let (account, phone, _, _) = two_devices(&app).await;
    set_push(&app, &phone, "ios", PHONE_TOKEN).await;
    set_push(&app, account.access(), "macos", LAPTOP_TOKEN).await;

    let (status, _) = call(
        &app,
        "POST",
        "/v1/approvals",
        None,
        Some(json!({ "email": "push@example.com", "commitment": b64(&[1u8; 32]), "platform": "ios" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let mut got = apns.wait_for(2).await;
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0, PHONE_TOKEN);
    assert_eq!(got[1].0, LAPTOP_TOKEN);
    assert_eq!(got[1].1["apns-topic"], "app.pwm.mac");
    for (_, headers, body) in &got {
        assert_eq!(headers["apns-push-type"], "alert");
        let body: Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["aps"]["alert"]["body"], APPROVAL_NOTICE);
        assert!(!body.to_string().contains("push@example.com"));
    }

    // An unknown email sends nothing, and gets the same answer.
    let (status, _) = call(
        &app,
        "POST",
        "/v1/approvals",
        None,
        Some(json!({ "email": "nobody@example.com", "commitment": b64(&[2u8; 32]), "platform": "ios" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(apns.wait_for(3).await.len(), 2);
    db.drop().await;
}

#[tokio::test]
async fn a_token_apns_refuses_is_deleted() {
    let Some((db, app, apns)) = setup().await else {
        return;
    };
    let (account, phone, vault, key) = two_devices(&app).await;
    set_push(&app, &phone, "ios", PHONE_TOKEN).await;
    apns.gone.lock().unwrap().push(PHONE_TOKEN.to_owned());
    write_item(&app, account.access(), &vault, &key).await;
    apns.wait_for(1).await;
    for _ in 0..50 {
        if push_rows(&db).await == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(push_rows(&db).await, 0);
    db.drop().await;
}

#[tokio::test]
async fn signing_out_or_revoking_a_device_deletes_its_token() {
    let Some((db, app, _)) = setup().await else {
        return;
    };
    let account = register(&app, "push@example.com").await;
    let phone_id = new_id();
    let (_, phone) = login(&app, &account, &phone_id).await;
    let phone = phone["access_token"].as_str().unwrap();
    set_push(&app, phone, "ios", PHONE_TOKEN).await;
    set_push(&app, account.access(), "macos", LAPTOP_TOKEN).await;
    assert_eq!(push_rows(&db).await, 2);

    let (status, _) = call(
        &app,
        "DELETE",
        &format!("/v1/devices/{phone_id}"),
        Some(account.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(push_rows(&db).await, 1);

    let (status, _) = call(
        &app,
        "POST",
        "/v1/auth/logout",
        Some(account.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(push_rows(&db).await, 0);
    db.drop().await;
}

// --- FCM (D60) ---

/// A synthetic FCM registration token: its shape, not a real one.
const FCM_TOKEN: &str = "dGVzdC1pbnN0YW5jZS1pZA:APA91bH_synthetic-token-for-tests_0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-_0123456789abcdefghijklmnopqrstuvwxyz";
/// A throwaway RSA key made for these tests, never used anywhere else.
const TEST_RSA_KEY: &str = include_str!("fixtures/fcm-test-key.pem");

type Messages = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

/// Records the token requests and the messages; answers 404 for tokens in `gone`.
#[derive(Clone, Default)]
struct Fcm {
    assertions: Arc<Mutex<Vec<String>>>,
    messages: Messages,
    gone: Arc<Mutex<Vec<String>>>,
}

impl Fcm {
    async fn start() -> (Self, String) {
        let fcm = Self::default();
        let tokens = fcm.clone();
        let sends = fcm.clone();
        let router = Router::new()
            .route(
                "/token",
                post(move |body: String| {
                    let tokens = tokens.clone();
                    async move {
                        tokens.assertions.lock().unwrap().push(body);
                        axum::Json(json!({ "access_token": "access-1", "expires_in": 3599, "token_type": "Bearer" }))
                    }
                }),
            )
            .route(
                "/v1/projects/{project}/messages:send",
                post(
                    move |Path(project): Path<String>, headers: HeaderMap, body: String| {
                        let sends = sends.clone();
                        async move {
                            assert_eq!(project, "pwm-test");
                            let body: Value = serde_json::from_str(&body).unwrap();
                            let token = body["message"]["token"].as_str().unwrap().to_owned();
                            let gone = sends.gone.lock().unwrap().contains(&token);
                            sends.messages.lock().unwrap().push((headers, body));
                            if gone {
                                (
                                    StatusCode::NOT_FOUND,
                                    r#"{"error":{"status":"NOT_FOUND","details":[{"errorCode":"UNREGISTERED"}]}}"#,
                                )
                            } else {
                                (StatusCode::OK, r#"{"name":"projects/pwm-test/messages/1"}"#)
                            }
                        }
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await });
        (fcm, url)
    }

    async fn wait_for(&self, n: usize) -> Vec<(HeaderMap, Value)> {
        for _ in 0..100 {
            if self.messages.lock().unwrap().len() >= n {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.messages.lock().unwrap().clone()
    }
}

async fn setup_fcm() -> Option<(TestDb, Router, Fcm)> {
    let db = migrated_db().await?;
    let (fcm, url) = Fcm::start().await;
    let credentials = json!({
        "type": "service_account",
        "project_id": "pwm-test",
        "client_email": "push@pwm-test.iam.gserviceaccount.com",
        "private_key": TEST_RSA_KEY,
    });
    let token_url = format!("{url}/token");
    let config = FcmConfig::new(&credentials.to_string(), Some(&url), Some(&token_url)).unwrap();
    let state = AppState::new(db.pool.clone(), ServerSecrets::generate().unwrap())
        .with_limits(Limits::unlimited(), system_clock())
        .with_push(Push::start(db.pool.clone(), None, Some(config)));
    Some((db, app(state), fcm))
}

async fn set_android(app: &Router, access: &str, token: &str, environment: &str) -> StatusCode {
    call(
        app,
        "PUT",
        "/v1/devices/self/push",
        Some(access),
        Some(json!({ "app": "android", "token": token, "environment": environment })),
    )
    .await
    .0
}

/// The test key's public half, for checking the assertion.
fn test_public_key() -> ring::signature::UnparsedPublicKey<Vec<u8>> {
    let body: String = TEST_RSA_KEY
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .unwrap();
    let pair = ring::signature::RsaKeyPair::from_pkcs8(&der).unwrap();
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        pair.public().as_ref().to_vec(),
    )
}

#[tokio::test]
async fn an_android_token_is_checked_and_kept_as_its_text() {
    let Some((db, app, _)) = setup_fcm().await else {
        return;
    };
    let account = register(&app, "push@example.com").await;
    let access = account.access();
    let spaced = format!("{} x", &FCM_TOKEN[..120]);
    for (token, environment) in [
        (FCM_TOKEN, "sandbox"),
        (&FCM_TOKEN[..99], "production"),
        (spaced.as_str(), "production"),
        (PHONE_TOKEN, "production"),
    ] {
        assert_eq!(
            set_android(&app, access, token, environment).await,
            StatusCode::BAD_REQUEST,
            "{token}"
        );
    }
    assert_eq!(
        set_android(&app, access, FCM_TOKEN, "production").await,
        StatusCode::NO_CONTENT
    );
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token FROM push_tokens")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(stored, FCM_TOKEN.as_bytes());
    db.drop().await;
}

#[tokio::test]
async fn fcm_gets_a_data_message_with_nothing_in_it_and_a_signed_assertion() {
    let Some((db, app, fcm)) = setup_fcm().await else {
        return;
    };
    let (account, phone, vault, key) = two_devices(&app).await;
    assert_eq!(
        set_android(&app, &phone, FCM_TOKEN, "production").await,
        StatusCode::NO_CONTENT
    );

    write_item(&app, account.access(), &vault, &key).await;
    let got = fcm.wait_for(1).await;
    assert_eq!(got.len(), 1);
    let (headers, body) = &got[0];
    assert_eq!(headers["authorization"], "Bearer access-1");
    assert_eq!(
        *body,
        json!({ "message": {
            "token": FCM_TOKEN,
            "data": { "t": "sync" },
            "android": { "priority": "normal", "ttl": "3600s", "collapse_key": "sync" },
        }})
    );
    assert_eq!(
        body["message"]["data"],
        serde_json::from_str::<Value>(FCM_SYNC_DATA).unwrap()
    );

    // The token request: the JWT-bearer grant, with an RS256 assertion from the service account.
    let assertions = fcm.assertions.lock().unwrap().clone();
    assert_eq!(assertions.len(), 1);
    let assertion = assertions[0]
        .strip_prefix("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion=")
        .unwrap();
    let parts: Vec<&str> = assertion.split('.').collect();
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(claims["iss"], "push@pwm-test.iam.gserviceaccount.com");
    assert_eq!(claims["scope"], FCM_SCOPE);
    assert!(claims["aud"].as_str().unwrap().ends_with("/token"));
    let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
    let signed = format!("{}.{}", parts[0], parts[1]);
    assert!(
        test_public_key()
            .verify(signed.as_bytes(), &signature)
            .is_ok()
    );

    // A new device's request: high priority, the kind only, and the access token reused.
    let (status, _) = call(
        &app,
        "POST",
        "/v1/approvals",
        None,
        Some(json!({ "email": "push@example.com", "commitment": b64(&[1u8; 32]), "platform": "android" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let got = fcm.wait_for(2).await;
    assert_eq!(got.len(), 2);
    assert_eq!(
        got[1].1["message"]["data"],
        serde_json::from_str::<Value>(FCM_APPROVAL_DATA).unwrap()
    );
    assert_eq!(got[1].1["message"]["android"]["priority"], "high");
    assert!(!got[1].1.to_string().contains("push@example.com"));
    assert_eq!(
        fcm.assertions.lock().unwrap().len(),
        1,
        "the access token is reused"
    );
    db.drop().await;
}

#[tokio::test]
async fn an_unregistered_fcm_token_is_deleted_and_apns_tokens_wait_for_apns() {
    let Some((db, app, fcm)) = setup_fcm().await else {
        return;
    };
    let (account, phone, vault, key) = two_devices(&app).await;
    set_android(&app, &phone, FCM_TOKEN, "production").await;
    // APNs is off on this server: the Mac's token is kept and sent nothing.
    set_push(&app, account.access(), "macos", LAPTOP_TOKEN).await;
    fcm.gone.lock().unwrap().push(FCM_TOKEN.to_owned());
    // The Mac wrote it: only the phone is asked.
    write_item(&app, account.access(), &vault, &key).await;
    fcm.wait_for(1).await;
    for _ in 0..50 {
        if push_rows(&db).await == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(push_rows(&db).await, 1);
    let left: String = sqlx::query_scalar("SELECT app FROM push_tokens")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(left, "macos");
    db.drop().await;
}

/// D74 §5: after deleting, the account's other devices get the content-free
/// push at once, even one pushed less than a minute ago; the deleting device
/// gets none.
#[tokio::test]
async fn a_deleted_accounts_other_devices_are_woken() {
    let Some((db, app, apns)) = setup().await else {
        return;
    };
    let (account, phone, vault, key) = two_devices(&app).await;
    assert_eq!(
        set_push(&app, account.access(), "macos", LAPTOP_TOKEN).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        set_push(&app, &phone, "ios", PHONE_TOKEN).await,
        StatusCode::NO_CONTENT
    );
    // The laptop writes: the phone is pushed now.
    write_item(&app, account.access(), &vault, &key).await;
    assert_eq!(apns.wait_for(1).await.len(), 1);

    let (status, body) = delete_account(&app, &account).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let got = apns.wait_for(2).await;
    assert_eq!(got.len(), 2, "the phone again, never the laptop");
    let (token, headers, body) = &got[1];
    assert_eq!(token, PHONE_TOKEN);
    assert_eq!(headers["apns-push-type"], "background");
    assert_eq!(body, BACKGROUND_PAYLOAD);
    assert_eq!(push_rows(&db).await, 0);
    db.drop().await;
}
