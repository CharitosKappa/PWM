//! Plans and billing (D6) against fake providers: one local HTTP server
//! answers as Stripe, the App Store Server API and the Play Developer API
//! do, from hand-written synthetic JSON. No real keys, no network.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
use axum::routing::{get, post};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use common::flows::{Account, register};
use common::{TestDb, call, migrated_db, send};
use hmac::{Hmac, KeyInit, Mac};
use p256::ecdsa::SigningKey;
use p256::pkcs8::{EncodePrivateKey, LineEnding};
use pwm_crypto::ServerSecrets;
use pwm_server::{
    AppState, AppleConfig, Billing, BillingConfig, Limits, Mailer, OutboxMessage, PlayConfig,
    StripeConfig, app, system_clock,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const WEBHOOK_SECRET: &str = "whsec_test";
const PLAY_SECRET: &str = "0123456789abcdef0123456789abcdef";
const BUNDLE: &str = "invalid.pwm.ios";
const APPLE_PRODUCT: &str = "invalid.pwm.premium.yearly";
const PACKAGE: &str = "invalid.pwm.android";
const TEST_RSA_KEY: &str = include_str!("fixtures/fcm-test-key.pem");
/// 2100-01-01, far ahead.
const LATER_SECS: i64 = 4_102_444_800;

#[derive(Default)]
struct Fake {
    stripe: HashMap<String, Value>,
    checkouts: Vec<HashMap<String, String>>,
    /// (sandbox, transaction id) → "Get All Subscription Statuses".
    apple: HashMap<(bool, String), Value>,
    play: HashMap<String, Value>,
    acknowledged: Vec<String>,
    calls: Vec<String>,
    /// Each refund's form and idempotency key.
    refunds: Vec<(HashMap<String, String>, Option<String>)>,
    /// Subscriptions moved to a new price: "sub_1 price_x".
    moves: Vec<String>,
    /// Play purchases revoked, with the request body.
    revoked: Vec<(String, String)>,
}

type Shared = Arc<Mutex<Fake>>;

fn form(body: &str) -> HashMap<String, String> {
    let decode = |s: &str| {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    };
    body.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap();
            (decode(k), decode(v))
        })
        .collect()
}

fn found(value: Option<Value>) -> (StatusCode, String) {
    match value {
        Some(v) => (StatusCode::OK, v.to_string()),
        None => (StatusCode::NOT_FOUND, "{}".to_owned()),
    }
}

async fn start_fake() -> (Shared, String) {
    let fake = Shared::default();
    let router = Router::new()
        .route(
            "/v1/customers",
            post(|State(f): State<Shared>, body: String| async move {
                let token = form(&body)["metadata[billing_token]"].clone();
                f.lock().unwrap().calls.push(format!("customer {token}"));
                json!({ "id": "cus_test1" }).to_string()
            }),
        )
        .route(
            "/v1/checkout/sessions",
            post(|State(f): State<Shared>, body: String| async move {
                f.lock().unwrap().checkouts.push(form(&body));
                json!({ "url": "https://checkout.stripe.test/c/1" }).to_string()
            }),
        )
        .route(
            "/v1/subscriptions/{id}",
            get(|State(f): State<Shared>, Path(id): Path<String>| async move {
                let mut f = f.lock().unwrap();
                f.calls.push(format!("stripe {id}"));
                found(f.stripe.get(&id).cloned())
            })
            .post(
                |State(f): State<Shared>, Path(id): Path<String>, body: String| async move {
                    let mut f = f.lock().unwrap();
                    let fields = form(&body);
                    if let Some(price) = fields.get("items[0][price]") {
                        // Applies at the next renewal, so the price shown stays.
                        assert_eq!(fields["items[0][id]"], "si_1");
                        assert_eq!(fields["proration_behavior"], "none");
                        f.moves.push(format!("{id} {price}"));
                        return found(f.stripe.get(&id).cloned());
                    }
                    let cancel = fields["cancel_at_period_end"] == "true";
                    let sub = f.stripe.get_mut(&id).map(|s| {
                        s["cancel_at_period_end"] = cancel.into();
                        s.clone()
                    });
                    found(sub)
                },
            )
            .delete(|State(f): State<Shared>, Path(id): Path<String>| async move {
                let mut f = f.lock().unwrap();
                f.calls.push(format!("end {id}"));
                let sub = f.stripe.get_mut(&id).map(|s| {
                    s["status"] = "canceled".into();
                    s.clone()
                });
                found(sub)
            }),
        )
        .route(
            "/v1/invoices/{id}",
            get(|Path(id): Path<String>| async move {
                assert_eq!(id, "in_1");
                json!({ "id": "in_1", "payments": { "data": [
                    { "payment": { "type": "payment_intent", "payment_intent": "pi_1" } }
                ] } })
                .to_string()
            }),
        )
        .route(
            "/v1/refunds",
            post(
                |State(f): State<Shared>, headers: axum::http::HeaderMap, body: String| async move {
                    let key = headers
                        .get("idempotency-key")
                        .map(|v| v.to_str().unwrap().to_owned());
                    let mut f = f.lock().unwrap();
                    f.calls.push("refund".into());
                    f.refunds.push((form(&body), key));
                    json!({ "id": "re_1", "status": "succeeded" }).to_string()
                },
            ),
        )
        .route(
            "/apple/inApps/v1/subscriptions/{id}",
            get(|State(f): State<Shared>, Path(id): Path<String>| async move {
                let mut f = f.lock().unwrap();
                f.calls.push(format!("apple {id}"));
                found(f.apple.get(&(false, id)).cloned())
            }),
        )
        .route(
            "/apple-sandbox/inApps/v1/subscriptions/{id}",
            get(|State(f): State<Shared>, Path(id): Path<String>| async move {
                let mut f = f.lock().unwrap();
                f.calls.push(format!("apple-sandbox {id}"));
                found(f.apple.get(&(true, id)).cloned())
            }),
        )
        .route(
            "/play/token",
            post(|| async { json!({ "access_token": "play-access", "expires_in": 3600 }).to_string() }),
        )
        .route(
            "/play/androidpublisher/v3/applications/{package}/purchases/subscriptionsv2/tokens/{token}",
            get(
                |State(f): State<Shared>, Path((package, token)): Path<(String, String)>| async move {
                    assert_eq!(package, PACKAGE);
                    let mut f = f.lock().unwrap();
                    f.calls.push(format!("play {token}"));
                    found(f.play.get(&token).cloned())
                },
            )
            .post(
                |State(f): State<Shared>, Path((_, rest)): Path<(String, String)>, body: String| async move {
                    let token = rest.strip_suffix(":revoke").unwrap().to_owned();
                    let mut f = f.lock().unwrap();
                    f.revoked.push((token.clone(), body));
                    if let Some(p) = f.play.get_mut(&token) {
                        p["subscriptionState"] = "SUBSCRIPTION_STATE_EXPIRED".into();
                    }
                    "{}"
                },
            ),
        )
        .route(
            "/play/androidpublisher/v3/applications/{package}/purchases/subscriptions/{product}/tokens/{rest}",
            post(
                |State(f): State<Shared>, Path((_, product, rest)): Path<(String, String, String)>| async move {
                    let token = rest.strip_suffix(":acknowledge").unwrap().to_owned();
                    let mut f = f.lock().unwrap();
                    f.acknowledged.push(format!("{product} {token}"));
                    if let Some(p) = f.play.get_mut(&token) {
                        p["acknowledgementState"] = "ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED".into();
                    }
                    "{}"
                },
            ),
        )
        .fallback(|uri: Uri| async move { (StatusCode::IM_A_TEAPOT, format!("the fake has no {uri}")) })
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    (fake, url)
}

fn config(url: &str) -> BillingConfig {
    let key = SigningKey::from_slice(&[9u8; 32]).unwrap();
    let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
    let credentials = json!({
        "type": "service_account",
        "client_email": "billing@pwm-test.iam.gserviceaccount.com",
        "private_key": TEST_RSA_KEY,
    })
    .to_string();
    BillingConfig {
        stripe: Some(
            StripeConfig::new(
                "sk_test_x",
                WEBHOOK_SECRET,
                "premium=price_p,family=price_f",
                "https://pwm.test",
                Some(url),
            )
            .unwrap(),
        ),
        apple: Some(
            AppleConfig::new(
                &pem,
                "KEY1234567",
                "57246542-96fe-1a63-e053-0824d011072a",
                BUNDLE,
                &format!("premium={APPLE_PRODUCT}"),
                Some((&format!("{url}/apple"), &format!("{url}/apple-sandbox"))),
            )
            .unwrap(),
        ),
        play: Some(
            PlayConfig::new(
                &credentials,
                PACKAGE,
                "premium=premium_yearly",
                PLAY_SECRET,
                Some((&format!("{url}/play"), &format!("{url}/play/token"))),
            )
            .unwrap(),
        ),
    }
}

struct World {
    db: TestDb,
    app: Router,
    fake: Shared,
    billing: Billing,
}

async fn world() -> Option<World> {
    world_with(None).await
}

async fn world_with(mailer: Option<Arc<dyn Mailer>>) -> Option<World> {
    let db = migrated_db().await?;
    let (fake, url) = start_fake().await;
    let mut billing = Billing::new(config(&url)).unwrap();
    if let Some(mailer) = mailer {
        billing = billing.with_mailer(mailer);
    }
    let state = AppState::new(db.pool.clone(), ServerSecrets::generate().unwrap())
        .with_limits(Limits::unlimited(), system_clock())
        .with_billing(billing.clone());
    Some(World {
        db,
        app: app(state),
        fake,
        billing,
    })
}

async fn token_of(app: &Router, account: &Account) -> String {
    let (status, body) = call(app, "GET", "/v1/billing", Some(account.access()), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

fn stripe_subscription(id: &str, token: &str, status: &str) -> Value {
    json!({
        "id": id, "object": "subscription", "status": status, "livemode": false,
        "cancel_at_period_end": false, "customer": "cus_test1",
        "metadata": { "billing_token": token },
        "items": { "data": [{ "id": "si_1", "current_period_end": LATER_SECS,
            "current_period_start": 1_700_000_000,
            "price": { "id": "price_p", "unit_amount": 2400, "currency": "usd" } }] },
        "latest_invoice": { "id": "in_1", "amount_paid": 2400,
            "status_transitions": { "paid_at": 1_700_000_000 } },
    })
}

fn stripe_signature(body: &str, at: u64) -> String {
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(WEBHOOK_SECRET.as_bytes()).unwrap();
    mac.update(format!("{at}.{body}").as_bytes());
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("t={at},v1={hex}")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn stripe_webhook(app: &Router, event: &Value, signature: Option<String>) -> StatusCode {
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let body = event.to_string();
    let signature = signature.unwrap_or_else(|| stripe_signature(&body, now_secs()));
    let request = axum::http::Request::post("/v1/billing/stripe/webhook")
        .header("stripe-signature", signature)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let _ = response.into_body().collect().await;
    status
}

fn jws(payload: &Value) -> String {
    format!(
        "eyJhbGciOiJFUzI1NiJ9.{}.c2ln",
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}

fn apple_statuses(status: i64, token: &str, auto_renew: i64) -> Value {
    let transaction = json!({
        "transactionId": "2000000002", "originalTransactionId": "2000000001",
        "bundleId": BUNDLE, "productId": APPLE_PRODUCT,
        "purchaseDate": 1_700_000_000_000i64, "expiresDate": LATER_SECS * 1000,
        "appAccountToken": token, "price": 23_990, "currency": "EUR",
    });
    json!({ "bundleId": BUNDLE, "environment": "Sandbox", "data": [{ "lastTransactions": [{
        "originalTransactionId": "2000000001", "status": status,
        "signedTransactionInfo": jws(&transaction),
        "signedRenewalInfo": jws(&json!({ "autoRenewStatus": auto_renew })),
    }]}]})
}

fn play_purchase(state: &str, token: &str) -> Value {
    json!({
        "subscriptionState": state, "testPurchase": {},
        "acknowledgementState": "ACKNOWLEDGEMENT_STATE_PENDING",
        "externalAccountIdentifiers": { "obfuscatedExternalAccountId": token },
        "lineItems": [{ "productId": "premium_yearly", "expiryTime": "2100-01-01T00:00:00Z",
            "autoRenewingPlan": { "autoRenewEnabled": true,
                "recurringPrice": { "currencyCode": "EUR", "units": "23", "nanos": 990_000_000 } } }],
    })
}

fn calls(fake: &Shared) -> usize {
    fake.lock().unwrap().calls.len()
}

#[tokio::test]
async fn without_billing_every_account_is_premium() {
    let Some((db, app)) = common::flows::setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let (status, body) = call(&app, "GET", "/v1/billing", Some(alice.access()), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["plan"], "premium");
    assert_eq!(body["billing"], json!([]));
    assert!(body["token"].as_str().unwrap().parse::<Uuid>().is_ok());
    for (path, body) in [
        ("/v1/billing/stripe/checkout", json!({ "plan": "premium" })),
        (
            "/v1/billing/apple/transactions",
            json!({ "transaction_id": "1" }),
        ),
    ] {
        let (status, answer) = call(&app, "POST", path, Some(alice.access()), Some(body)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert_eq!(answer["error"], "billing_unavailable");
    }
    let (status, _) = call(
        &app,
        "POST",
        "/v1/billing/apple/notifications",
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    db.drop().await;
}

#[tokio::test]
async fn stripe_checkout_webhook_cancel_and_resume() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "free");
    assert_eq!(me["billing"], json!(["stripe", "app_store", "play"]));
    let token = token_of(&w.app, &alice).await;

    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/checkout",
        Some(alice.access()),
        Some(json!({ "plan": "gold" })),
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::BAD_REQUEST, &json!("invalid_request"))
    );
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/checkout",
        Some(alice.access()),
        Some(json!({ "plan": "premium" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["url"], "https://checkout.stripe.test/c/1");
    {
        let fake = w.fake.lock().unwrap();
        let checkout = &fake.checkouts[0];
        assert_eq!(checkout["customer"], "cus_test1");
        assert_eq!(checkout["client_reference_id"], token);
        assert_eq!(
            checkout["subscription_data[metadata][billing_token]"],
            token
        );
        assert_eq!(checkout["line_items[0][price]"], "price_p");
        assert_eq!(checkout["success_url"], "https://pwm.test/billing/done");
        assert_eq!(fake.calls, [format!("customer {token}")]);
    }
    let stored: Option<String> =
        sqlx::query_scalar("SELECT stripe_customer FROM accounts WHERE id = $1")
            .bind(alice.id)
            .fetch_one(&w.db.pool)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some("cus_test1"));

    // Paid: Stripe sends checkout.session.completed.
    w.fake.lock().unwrap().stripe.insert(
        "sub_1".into(),
        stripe_subscription("sub_1", &token, "active"),
    );
    let event = json!({ "id": "evt_1", "type": "checkout.session.completed",
        "data": { "object": { "object": "checkout.session", "subscription": "sub_1" } } });
    assert_eq!(stripe_webhook(&w.app, &event, None).await, StatusCode::OK);
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "premium");
    assert_eq!(me["subscription"]["provider"], "stripe");
    assert_eq!(me["subscription"]["until"], LATER_SECS * 1000);
    assert_eq!(me["subscription"]["canceling"], false);

    // The same event again is dropped; a bad or old signature is refused.
    let before = calls(&w.fake);
    assert_eq!(stripe_webhook(&w.app, &event, None).await, StatusCode::OK);
    assert_eq!(calls(&w.fake), before);
    let other =
        json!({ "id": "evt_2", "data": { "object": { "object": "subscription", "id": "sub_1" } } });
    assert_eq!(
        stripe_webhook(&w.app, &other, Some("t=1,v1=00".into())).await,
        StatusCode::BAD_REQUEST
    );
    let old = stripe_signature(&other.to_string(), now_secs() - 600);
    assert_eq!(
        stripe_webhook(&w.app, &other, Some(old)).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(calls(&w.fake), before);

    // A second checkout isn't sold.
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/checkout",
        Some(alice.access()),
        Some(json!({ "plan": "premium" })),
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::CONFLICT, &json!("already_subscribed"))
    );

    // One click to cancel, one to resume.
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/cancel",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"], "premium");
    assert_eq!(body["subscription"]["canceling"], true);
    let (_, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/resume",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(body["subscription"]["canceling"], false);

    // Stripe ends it: the plan goes back to Free, nothing else changes.
    w.fake.lock().unwrap().stripe.get_mut("sub_1").unwrap()["status"] = "canceled".into();
    let ended = json!({ "id": "evt_3", "type": "customer.subscription.deleted",
        "data": { "object": { "object": "subscription", "id": "sub_1" } } });
    assert_eq!(stripe_webhook(&w.app, &ended, None).await, StatusCode::OK);
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "free");
    assert_eq!(me["subscription"], json!(null));
    let (status, _) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/cancel",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    w.db.drop().await;
}

#[tokio::test]
async fn an_apple_purchase_is_linked_by_its_token_only() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let bob = register(&w.app, "bob@example.com").await;
    let token = token_of(&w.app, &alice).await;
    // Bought in the sandbox, as App Review and TestFlight do.
    w.fake
        .lock()
        .unwrap()
        .apple
        .insert((true, "2000000002".into()), apple_statuses(1, &token, 1));

    let purchase = json!({ "transaction_id": "2000000002" });
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/apple/transactions",
        Some(bob.access()),
        Some(purchase.clone()),
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::FORBIDDEN, &json!("purchase_not_yours"))
    );
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/apple/transactions",
        Some(alice.access()),
        Some(purchase),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"], "premium");
    assert_eq!(body["subscription"]["provider"], "app_store");
    assert_eq!(
        w.fake.lock().unwrap().calls[..2],
        ["apple 2000000002", "apple-sandbox 2000000002"]
    );
    let row: (String, bool, i64, String) = sqlx::query_as(
        "SELECT provider_ref, sandbox, price_minor, currency FROM subscriptions WHERE account_id = $1",
    )
    .bind(alice.id)
    .fetch_one(&w.db.pool)
    .await
    .unwrap();
    assert_eq!(row, ("2000000001".into(), true, 2399, "EUR".into()));
    let (_, bob_me) = call(&w.app, "GET", "/v1/account", Some(bob.access()), None).await;
    assert_eq!(bob_me["plan"], "free");

    // Cancel and resume are the App Store's own.
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/cancel",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::CONFLICT, &json!("managed_elsewhere"))
    );

    // A notification only says where to look: the answer comes from Apple.
    w.fake
        .lock()
        .unwrap()
        .apple
        .insert((true, "2000000001".into()), apple_statuses(2, &token, 0));
    let notification = |id: &str, bundle: &str| {
        json!({ "signedPayload": jws(&json!({
            "notificationUUID": id, "notificationType": "EXPIRED",
            "data": { "bundleId": bundle, "environment": "Sandbox",
                "signedTransactionInfo": jws(&json!({ "originalTransactionId": "2000000001" })) },
        })) })
    };
    let before = calls(&w.fake);
    let (status, _) = call(
        &w.app,
        "POST",
        "/v1/billing/apple/notifications",
        None,
        Some(notification("n-0", "com.someone.else")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        calls(&w.fake),
        before,
        "another app's notification is ignored"
    );
    let (status, _) = call(
        &w.app,
        "POST",
        "/v1/billing/apple/notifications",
        None,
        Some(notification("n-1", BUNDLE)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        w.fake.lock().unwrap().calls.last().unwrap(),
        "apple-sandbox 2000000001"
    );
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "free");
    w.db.drop().await;
}

#[tokio::test]
async fn a_play_purchase_is_read_acknowledged_and_replaced() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    let first = "play.token-0001";
    w.fake.lock().unwrap().play.insert(
        first.into(),
        play_purchase("SUBSCRIPTION_STATE_PENDING", &token),
    );
    let buy = |purchase_token: &str| json!({ "product_id": "premium_yearly", "purchase_token": purchase_token });
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/play/purchases",
        Some(alice.access()),
        Some(buy(first)),
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::CONFLICT, &json!("purchase_pending"))
    );

    w.fake.lock().unwrap().play.insert(
        first.into(),
        play_purchase("SUBSCRIPTION_STATE_ACTIVE", &token),
    );
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/play/purchases",
        Some(alice.access()),
        Some(buy(first)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["subscription"]["provider"], "play");
    assert_eq!(
        w.fake.lock().unwrap().acknowledged,
        [format!("premium_yearly {first}")]
    );

    // A resubscribe replaces the old purchase, which stops counting.
    let second = "play.token-0002";
    let mut renewed = play_purchase("SUBSCRIPTION_STATE_ACTIVE", &token);
    renewed["linkedPurchaseToken"] = first.into();
    w.fake.lock().unwrap().play.insert(second.into(), renewed);
    let push = |data: Value| {
        json!({ "message": { "messageId": "m-1", "data": STANDARD.encode(data.to_string()) },
            "subscription": "projects/p/subscriptions/s" })
    };
    let notice = push(json!({ "packageName": PACKAGE,
        "subscriptionNotification": { "purchaseToken": second, "notificationType": 4 } }));
    let (status, _) = call(
        &w.app,
        "POST",
        "/v1/billing/play/notifications/wrong-secret-wrong-secret-wrong-sec",
        None,
        Some(notice.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &w.app,
        "POST",
        &format!("/v1/billing/play/notifications/{PLAY_SECRET}"),
        None,
        Some(notice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT provider_ref, status FROM subscriptions WHERE account_id = $1 ORDER BY provider_ref",
    )
    .bind(alice.id)
    .fetch_all(&w.db.pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            (first.to_owned(), "expired".to_owned()),
            (second.to_owned(), "active".to_owned())
        ]
    );
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "premium");
    assert_eq!(me["subscription"]["also"], json!([]));
    w.db.drop().await;
}

#[tokio::test]
async fn two_subscriptions_count_and_the_second_is_pointed_out() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    {
        let mut fake = w.fake.lock().unwrap();
        fake.stripe.insert(
            "sub_9".into(),
            stripe_subscription("sub_9", &token, "active"),
        );
        fake.apple
            .insert((false, "2000000002".into()), apple_statuses(1, &token, 1));
    }
    let event =
        json!({ "id": "evt_9", "data": { "object": { "object": "subscription", "id": "sub_9" } } });
    assert_eq!(stripe_webhook(&w.app, &event, None).await, StatusCode::OK);
    let (status, _) = call(
        &w.app,
        "POST",
        "/v1/billing/apple/transactions",
        Some(alice.access()),
        Some(json!({ "transaction_id": "2000000002" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "premium");
    let first = me["subscription"]["provider"].as_str().unwrap();
    let also = me["subscription"]["also"].as_array().unwrap();
    assert_eq!(also.len(), 1);
    assert_ne!(also[0], first);
    w.db.drop().await;
}

async fn seed_subscription(
    pool: &PgPool,
    account: Uuid,
    reference: &str,
    status: &str,
    extra: &str,
) {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO subscriptions (account_id, plan, status, provider, provider_ref, price_minor, currency, \
            current_period_end, grace_until) \
         VALUES ($1, 'premium', '{status}', 'stripe', '{reference}', 2400, 'USD', {extra})"
    )))
    .bind(account)
    .execute(pool)
    .await
    .unwrap();
}

/// D6 §4 over every status, straight from the table.
#[tokio::test]
async fn the_plan_rule_covers_every_status() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let plan = |label: &'static str| {
        let app = w.app.clone();
        let access = alice.access().to_owned();
        async move {
            let (_, me) = call(&app, "GET", "/v1/account", Some(&access), None).await;
            (label, me["plan"].as_str().unwrap().to_owned())
        }
    };
    let cases = [
        ("active", "now() + interval '1 day', NULL", "premium"),
        ("active", "now() - interval '2 days', NULL", "premium"),
        ("active", "now() - interval '4 days', NULL", "free"),
        ("canceled", "now() + interval '1 day', NULL", "premium"),
        ("canceled", "now() - interval '1 minute', NULL", "free"),
        (
            "past_due",
            "now() - interval '1 day', now() + interval '1 day'",
            "premium",
        ),
        (
            "past_due",
            "now() + interval '1 day', now() - interval '1 minute'",
            "free",
        ),
        ("past_due", "now() + interval '1 day', NULL", "premium"),
        ("expired", "now() + interval '1 day', NULL", "free"),
    ];
    for (i, (status, extra, expected)) in cases.into_iter().enumerate() {
        sqlx::query("DELETE FROM subscriptions")
            .execute(&w.db.pool)
            .await
            .unwrap();
        seed_subscription(&w.db.pool, alice.id, &format!("sub_{i}"), status, extra).await;
        assert_eq!(plan("case").await.1, expected, "{status} {extra}");
    }
    w.db.drop().await;
}

/// The daily pass reads every live subscription again and ends the ones
/// their provider no longer knows.
#[tokio::test]
async fn the_daily_pass_reads_every_subscription_again() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    seed_subscription(
        &w.db.pool,
        alice.id,
        "sub_live",
        "active",
        "now() + interval '1 day', NULL",
    )
    .await;
    seed_subscription(
        &w.db.pool,
        alice.id,
        "sub_gone",
        "active",
        "now() + interval '1 day', NULL",
    )
    .await;
    let mut ended = stripe_subscription("sub_live", &token, "canceled");
    ended["metadata"] = json!({});
    w.fake
        .lock()
        .unwrap()
        .stripe
        .insert("sub_live".into(), ended);
    sqlx::query("INSERT INTO billing_events (provider, event_id, received_at) VALUES ('stripe', 'old', now() - interval '31 days'), ('stripe', 'new', now())")
        .execute(&w.db.pool)
        .await
        .unwrap();

    w.billing.reconcile(&w.db.pool).await.unwrap();
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT provider_ref, status FROM subscriptions ORDER BY provider_ref")
            .fetch_all(&w.db.pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [
            ("sub_gone".to_owned(), "expired".to_owned()),
            ("sub_live".to_owned(), "expired".to_owned())
        ]
    );
    let events: Vec<String> = sqlx::query_scalar("SELECT event_id FROM billing_events")
        .fetch_all(&w.db.pool)
        .await
        .unwrap();
    assert_eq!(events, ["new"]);
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "free");
    w.db.drop().await;
}

#[tokio::test]
async fn the_return_pages_say_to_go_back_to_the_app() {
    let Some(w) = world().await else { return };
    for path in ["/billing/done", "/billing/canceled"] {
        let reply = send(&w.app, "GET", path, None, &[], None).await;
        assert_eq!(reply.status, StatusCode::OK);
        assert!(reply.body.as_str().unwrap().contains("go back to PWM"));
        assert_eq!(reply.headers["cache-control"], "no-store");
    }
    w.db.drop().await;
}

/// The app API (`pwm-client`) end to end: the plan, Checkout, cancel and
/// resume, and a store purchase another account made (D6 §7).
#[tokio::test]
async fn the_app_api_reads_and_changes_the_plan() {
    use common::driver::run;
    use pwm_client::{Client, KdfParams, MemoryStore, Outcome, Platform, new_id};

    let Some(w) = world().await else { return };
    let mut client = Client::new(MemoryStore::new());
    client.set_signup_params(KdfParams::MOBILE_FLOOR).unwrap();
    let step = client.start_signup(
        "alice@example.com",
        "correct horse battery staple 42",
        &new_id().unwrap(),
        Platform::Macos,
        "Mac",
    );
    assert!(matches!(
        run(&w.app, &mut client, step).await.unwrap(),
        Outcome::SignedUp { .. }
    ));

    let step = client.start_billing_status();
    let Outcome::Billing(info) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("no plan");
    };
    assert_eq!(info.plan, "free");
    assert_eq!(info.sells, ["stripe", "app_store", "play"]);
    assert!(info.subscription.is_none());
    let token = info.token.unwrap();

    let step = client.start_checkout("premium");
    let Outcome::CheckoutUrl(url) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("no checkout");
    };
    assert_eq!(url, "https://checkout.stripe.test/c/1");
    let step = client.start_cancel_subscription();
    assert_eq!(
        run(&w.app, &mut client, step).await.unwrap_err().code(),
        "no_subscription"
    );

    w.fake.lock().unwrap().stripe.insert(
        "sub_c".into(),
        stripe_subscription("sub_c", &token, "active"),
    );
    let event =
        json!({ "id": "evt_c", "data": { "object": { "object": "subscription", "id": "sub_c" } } });
    assert_eq!(stripe_webhook(&w.app, &event, None).await, StatusCode::OK);
    let step = client.start_cancel_subscription();
    let Outcome::Billing(info) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("not canceled");
    };
    let subscription = info.subscription.unwrap();
    assert_eq!(subscription.provider, "stripe");
    assert!(subscription.canceling);
    assert_eq!(subscription.until_ms, LATER_SECS * 1000);
    assert_eq!(info.token, None);
    let step = client.start_resume_subscription();
    let Outcome::Billing(info) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("not resumed");
    };
    assert!(!info.subscription.unwrap().canceling);
    let step = client.start_checkout("premium");
    assert_eq!(
        run(&w.app, &mut client, step).await.unwrap_err().code(),
        "already_subscribed"
    );

    // Someone else's App Store purchase isn't taken.
    w.fake.lock().unwrap().apple.insert(
        (false, "3000000001".into()),
        apple_statuses(1, &Uuid::new_v4().to_string(), 1),
    );
    let step = client.start_apple_purchase("3000000001");
    assert_eq!(
        run(&w.app, &mut client, step).await.unwrap_err().code(),
        "purchase_not_yours"
    );
    w.db.drop().await;
}

// --- The pricing commitments (D72) ---

const DAY: i64 = 24 * 3600;

/// Records each message it is given, and says each was sent.
#[derive(Default)]
struct Recorder(Mutex<Vec<OutboxMessage>>);

impl Mailer for Recorder {
    fn deliver<'a>(
        &'a self,
        message: &'a OutboxMessage,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        self.0.lock().unwrap().push(message.clone());
        Box::pin(async { true })
    }
}

impl Recorder {
    fn kinds(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.kind.clone())
            .collect()
    }
}

/// A Stripe subscription for `token`, paid `paid_days_ago` days ago for a
/// period from then to `ends_in_days` from now, stored through a webhook.
async fn paying(w: &World, token: &str, id: &str, paid_days_ago: i64, ends_in_days: i64) {
    let now = now_secs() as i64;
    let mut sub = stripe_subscription(id, token, "active");
    sub["items"]["data"][0]["current_period_start"] = (now - paid_days_ago * DAY).into();
    sub["items"]["data"][0]["current_period_end"] = (now + ends_in_days * DAY).into();
    sub["latest_invoice"]["status_transitions"]["paid_at"] = (now - paid_days_ago * DAY).into();
    w.fake.lock().unwrap().stripe.insert(id.into(), sub);
    let event = json!({ "id": format!("evt_{id}"),
        "data": { "object": { "object": "subscription", "id": id } } });
    assert_eq!(stripe_webhook(&w.app, &event, None).await, StatusCode::OK);
}

#[tokio::test]
async fn a_stripe_refund_pays_back_the_unused_days_once_and_ends_the_plan() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    paying(&w, &token, "sub_r", 5, 360).await;
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    let refundable = me["subscription"]["refundable_until"].as_i64().unwrap();
    let expected = (now_secs() as i64 + 25 * DAY) * 1000;
    assert!((refundable - expected).abs() < 60_000, "{me}");

    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/refund",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"], "free");
    assert_eq!(body["subscription"], json!(null));
    {
        let fake = w.fake.lock().unwrap();
        assert_eq!(fake.refunds.len(), 1);
        let (refund, key) = &fake.refunds[0];
        assert_eq!(refund["payment_intent"], "pi_1");
        // 2400 × 360 / 365, rounded down; a second may pass meanwhile.
        let amount: i64 = refund["amount"].parse().unwrap();
        assert!((2366..=2367).contains(&amount), "{amount}");
        assert_eq!(key.as_deref(), Some("pwm-refund-in_1"));
        let refunded_at = fake.calls.iter().position(|c| c == "refund").unwrap();
        let ended_at = fake.calls.iter().position(|c| c == "end sub_r").unwrap();
        assert!(refunded_at < ended_at, "the refund goes first");
    }
    // Nothing left to refund.
    let (status, _) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/refund",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(w.fake.lock().unwrap().refunds.len(), 1);
    w.db.drop().await;
}

#[tokio::test]
async fn after_thirty_days_a_refund_is_refused_plainly() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    paying(&w, &token, "sub_old", 31, 334).await;
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["subscription"]["refundable_until"], json!(null));
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/stripe/refund",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::CONFLICT, &json!("refund_window_passed"))
    );
    assert!(w.fake.lock().unwrap().refunds.is_empty());
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(me["plan"], "premium");
    // A Play refund for a Stripe plan is someone else's to make.
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/play/refund",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::CONFLICT, &json!("managed_elsewhere"))
    );
    w.db.drop().await;
}

#[tokio::test]
async fn a_play_refund_is_pro_rata_through_google() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    let purchase_token = "play.token-r001";
    w.fake.lock().unwrap().play.insert(
        purchase_token.into(),
        play_purchase("SUBSCRIPTION_STATE_ACTIVE", &token),
    );
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/play/purchases",
        Some(alice.access()),
        Some(json!({ "product_id": "premium_yearly", "purchase_token": purchase_token })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // Bought just now: the window is open.
    assert!(body["subscription"]["refundable_until"].is_i64(), "{body}");

    // Thirty-one days on, it's closed.
    sqlx::query("UPDATE subscriptions SET last_charge_at = now() - interval '31 days'")
        .execute(&w.db.pool)
        .await
        .unwrap();
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/play/refund",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(
        (status, &body["error"]),
        (StatusCode::CONFLICT, &json!("refund_window_passed"))
    );

    sqlx::query("UPDATE subscriptions SET last_charge_at = now() - interval '2 days'")
        .execute(&w.db.pool)
        .await
        .unwrap();
    let (status, body) = call(
        &w.app,
        "POST",
        "/v1/billing/play/refund",
        Some(alice.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"], "free");
    let revoked = w.fake.lock().unwrap().revoked.clone();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].0, purchase_token);
    let sent: Value = serde_json::from_str(&revoked[0].1).unwrap();
    assert_eq!(
        sent,
        json!({ "revocationContext": { "proratedRefund": {} } })
    );
    w.db.drop().await;
}

#[tokio::test]
async fn a_price_change_holds_subscribers_and_moves_them_only_after_a_delivered_notice() {
    let mailer = Arc::new(Recorder::default());
    let Some(w) = world_with(Some(mailer.clone())).await else {
        return;
    };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    paying(&w, &token, "sub_h", 3, 362).await;

    let held =
        pwm_server::record_price_change(&w.db.pool, "stripe", "premium", "price_p2", 3000, "usd")
            .await
            .unwrap();
    assert_eq!(held, 1);
    let (_, me) = call(&w.app, "GET", "/v1/account", Some(alice.access()), None).await;
    let locked = me["subscription"]["price_locked_until"].as_i64().unwrap();
    let year = (now_secs() as i64 + 365 * DAY) * 1000;
    assert!((locked - year).abs() < 2 * DAY * 1000, "{me}");
    // Bad input is refused before anything is written.
    for (provider, plan, price, amount, currency) in [
        ("paypal", "premium", "price_x", 1, "USD"),
        ("stripe", "gold", "price_x", 1, "USD"),
        ("stripe", "premium", "price x", 1, "USD"),
        ("stripe", "premium", "price_x", -1, "USD"),
        ("stripe", "premium", "price_x", 1, "US"),
    ] {
        assert!(
            pwm_server::record_price_change(&w.db.pool, provider, plan, price, amount, currency)
                .await
                .is_err()
        );
    }

    // The notice goes now; the reminder waits for 60 days before the hold ends.
    w.billing.reconcile(&w.db.pool).await.unwrap();
    assert_eq!(mailer.kinds(), ["price_change"]);
    {
        let messages = mailer.0.lock().unwrap();
        assert_eq!(messages[0].email, "alice@example.com");
        assert_eq!(
            (messages[0].price_minor, messages[0].currency.as_str()),
            (3000, "USD")
        );
        assert_eq!(messages[0].about_ms / 1000, locked / 1000);
    }
    assert!(w.fake.lock().unwrap().moves.is_empty(), "still held");

    // The hold is over, but the reminder was never delivered: no move.
    sqlx::query("UPDATE subscriptions SET price_locked_until = now() - interval '1 day'")
        .execute(&w.db.pool)
        .await
        .unwrap();
    w.billing.reconcile(&w.db.pool).await.unwrap();
    assert!(
        w.fake.lock().unwrap().moves.is_empty(),
        "no delivered notice"
    );

    // Delivered 59 days ago: still no move. 61 days: it moves.
    for (days, moves) in [(59, 0), (61, 1)] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE billing_outbox SET sent_at = now() - interval '{days} days' \
             WHERE kind = 'price_change_reminder'"
        )))
        .execute(&w.db.pool)
        .await
        .unwrap();
        w.billing.reconcile(&w.db.pool).await.unwrap();
        assert_eq!(w.fake.lock().unwrap().moves.len(), moves, "{days} days");
    }
    assert_eq!(w.fake.lock().unwrap().moves, ["sub_h price_p2"]);
    w.db.drop().await;
}

#[tokio::test]
async fn renewal_reminders_go_once_fourteen_days_before_each_charge() {
    let mailer = Arc::new(Recorder::default());
    let Some(w) = world_with(Some(mailer.clone())).await else {
        return;
    };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    paying(&w, &token, "sub_soon", 355, 10).await;
    paying(&w, &token, "sub_later", 345, 20).await;
    paying(&w, &token, "sub_ending", 355, 10).await;
    w.fake.lock().unwrap().stripe.get_mut("sub_ending").unwrap()["cancel_at_period_end"] =
        true.into();

    for _ in 0..2 {
        w.billing.reconcile(&w.db.pool).await.unwrap();
    }
    let messages = mailer.0.lock().unwrap().clone();
    assert_eq!(messages.len(), 1, "{messages:?}");
    let reminder = &messages[0];
    assert_eq!(reminder.kind, "renewal_reminder");
    assert_eq!(reminder.provider, "stripe");
    assert_eq!(
        (reminder.price_minor, reminder.currency.as_str()),
        (2400, "USD")
    );
    let soon = w.fake.lock().unwrap().stripe["sub_soon"]["items"]["data"][0]["current_period_end"]
        .as_i64()
        .unwrap();
    assert_eq!(reminder.about_ms, soon * 1000);
    // The later one is scheduled for its own day, not sent yet.
    let waiting: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_outbox WHERE provider_ref = 'sub_later' AND sent_at IS NULL \
         AND send_after > now() + interval '5 days'",
    )
    .fetch_one(&w.db.pool)
    .await
    .unwrap();
    assert_eq!(waiting, 1);
    w.db.drop().await;
}

/// Without an email provider (D32) nothing is sent: the outbox keeps it.
#[tokio::test]
async fn without_a_mailer_the_outbox_waits() {
    let Some(w) = world().await else { return };
    let alice = register(&w.app, "alice@example.com").await;
    let token = token_of(&w.app, &alice).await;
    paying(&w, &token, "sub_w", 355, 10).await;
    w.billing.reconcile(&w.db.pool).await.unwrap();
    let unsent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM billing_outbox WHERE kind = 'renewal_reminder' AND sent_at IS NULL",
    )
    .fetch_one(&w.db.pool)
    .await
    .unwrap();
    assert_eq!(unsent, 1);
    w.db.drop().await;
}

/// A lapsed account keeps everything and can still read, edit and export
/// it (PRICING.md §7, D72 §4).
#[tokio::test]
async fn a_lapsed_account_keeps_its_data_and_still_exports() {
    use common::driver::{T0, run};
    use pwm_client::{Client, ExportFormat, KdfParams, MemoryStore, Outcome, Platform, new_id};
    use pwm_vault::{Item, ItemId, ItemKind};

    let Some(w) = world().await else { return };
    let mut client = Client::new(MemoryStore::new());
    client.set_signup_params(KdfParams::MOBILE_FLOOR).unwrap();
    let password = "correct horse battery staple 42";
    let step = client.start_signup(
        "alice@example.com",
        password,
        &new_id().unwrap(),
        Platform::Linux,
        "Laptop",
    );
    let Outcome::SignedUp { secret_key } = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("not signed up");
    };
    let step = client.start_ensure_vault();
    let Outcome::VaultCreated { vault_id } = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("no vault");
    };
    let id = ItemId::generate().unwrap();
    let item = Item::new(ItemKind::Note, "Bank", T0)
        .to_json_with_id(&id)
        .unwrap();
    client
        .save_item(&vault_id, item.expose_secret(), T0)
        .unwrap();
    let step = client.start_sync();
    run(&w.app, &mut client, step).await.unwrap();

    // The plan lapsed.
    let account: Uuid = client.account_id().unwrap().unwrap().parse().unwrap();
    seed_subscription(
        &w.db.pool,
        account,
        "sub_lapsed",
        "expired",
        "now() - interval '1 day', NULL",
    )
    .await;
    let step = client.start_billing_status();
    let Outcome::Billing(info) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("no plan");
    };
    assert_eq!(info.plan, "free");

    // Still syncs and edits, and nothing was deleted on the server.
    let edited = Item::new(ItemKind::Note, "Bank (old)", T0 + 1)
        .to_json_with_id(&id)
        .unwrap();
    client
        .save_item(&vault_id, edited.expose_secret(), T0 + 1)
        .unwrap();
    let step = client.start_sync();
    let Outcome::Synced(report) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("not synced");
    };
    assert_eq!(report.pushed, 1);
    let items: i64 = sqlx::query_scalar("SELECT count(*) FROM items")
        .fetch_one(&w.db.pool)
        .await
        .unwrap();
    assert_eq!(items, 1);

    // And exports.
    client
        .authorize_export(password, secret_key.expose_secret(), T0)
        .unwrap();
    let file = client.export(ExportFormat::Json, T0 + 2).unwrap();
    assert!(String::from_utf8_lossy(file.expose_secret()).contains("Bank (old)"));
    w.db.drop().await;
}

/// The refund through the app API (D72 §1).
#[tokio::test]
async fn the_app_api_refunds_and_ends_the_plan() {
    use common::driver::run;
    use pwm_client::{Client, KdfParams, MemoryStore, Outcome, Platform, new_id};

    let Some(w) = world().await else { return };
    let mut client = Client::new(MemoryStore::new());
    client.set_signup_params(KdfParams::MOBILE_FLOOR).unwrap();
    let step = client.start_signup(
        "alice@example.com",
        "correct horse battery staple 42",
        &new_id().unwrap(),
        Platform::Windows,
        "Desk",
    );
    run(&w.app, &mut client, step).await.unwrap();
    let step = client.start_billing_status();
    let Outcome::Billing(info) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("no plan");
    };
    paying(&w, &info.token.unwrap(), "sub_c", 2, 363).await;

    let step = client.start_billing_status();
    let Outcome::Billing(info) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("no plan");
    };
    assert!(info.subscription.unwrap().refundable_until_ms.is_some());
    assert_eq!(
        client.start_refund("app_store").unwrap_err().code(),
        "managed_elsewhere"
    );
    let step = client.start_refund("stripe");
    let Outcome::Billing(info) = run(&w.app, &mut client, step).await.unwrap() else {
        panic!("not refunded");
    };
    assert_eq!(info.plan, "free");
    assert_eq!(w.fake.lock().unwrap().refunds.len(), 1);
    w.db.drop().await;
}
