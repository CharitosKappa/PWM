//! Size limits, count caps and rate limits (D29), through the HTTP API
//! against a real Postgres.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use common::flows::*;
use common::{Reply, call, migrated_db, send, send_from};
use pwm_crypto::{ServerSecrets, SymmetricKey};
use pwm_server::{AppState, Limits, MAX_ITEM_BLOB_BYTES, ManualClock, Rate, TrustedProxies, app};
use serde_json::{Value, json};
use uuid::Uuid;

fn error(code: &str) -> Value {
    json!({ "error": code })
}

async fn create_vault(app: &Router, account: &Account) -> Uuid {
    let id = new_id();
    let key = SymmetricKey::generate().unwrap();
    let wrapped = account.keyset.wrap_vault_key(&key, id.as_bytes()).unwrap();
    let (status, body) = call(
        app,
        "POST",
        "/v1/vaults",
        Some(account.access()),
        Some(json!({ "id": id, "wrapped_key": b64(&wrapped.to_bytes()) })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    id
}

/// A blob that parses as a v1 envelope, `len` bytes long.
fn blob(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    bytes[0] = 1;
    b64(&bytes)
}

async fn put_new(app: &Router, token: &str, vault: &Uuid, item: &Uuid, blob: String) -> Reply {
    send(
        app,
        "PUT",
        &format!("/v1/vaults/{vault}/items/{item}"),
        Some(token),
        &[("if-none-match", "*")],
        Some(json!({ "revision": "1", "blob": blob })),
    )
    .await
}

#[tokio::test]
async fn bodies_and_blobs_have_size_limits() {
    assert_eq!(MAX_ITEM_BLOB_BYTES, pwm_vault::MAX_SEALED_ITEM_BYTES);
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let token = account.access();
    let vault = create_vault(&app, &account).await;

    // Exactly at the limit is fine; one byte over is not.
    let reply = put_new(&app, token, &vault, &new_id(), blob(MAX_ITEM_BLOB_BYTES)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    let reply = put_new(
        &app,
        token,
        &vault,
        &new_id(),
        blob(MAX_ITEM_BLOB_BYTES + 1),
    )
    .await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::PAYLOAD_TOO_LARGE, error("too_large"))
    );
    // A body over 384 KiB is refused before it is parsed.
    let huge = "A".repeat(400 * 1024);
    let reply = put_new(&app, token, &vault, &new_id(), huge).await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::PAYLOAD_TOO_LARGE, error("too_large"))
    );
    // Other endpoints take at most 16 KiB.
    let padded =
        json!({ "id": new_id(), "wrapped_key": b64(&[1u8; 41]), "pad": "x".repeat(16 * 1024) });
    let (status, body) = call(&app, "POST", "/v1/vaults", Some(token), Some(padded)).await;
    assert_eq!(
        (status, body),
        (StatusCode::PAYLOAD_TOO_LARGE, error("too_large"))
    );
    let padded = json!({ "email": "x@example.com", "pad": "x".repeat(16 * 1024) });
    let (status, body) = call(&app, "POST", "/v1/auth/prelogin", None, Some(padded)).await;
    assert_eq!(
        (status, body),
        (StatusCode::PAYLOAD_TOO_LARGE, error("too_large"))
    );
    db.drop().await;
}

#[tokio::test]
async fn vault_and_item_counts_are_capped() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let token = account.access();
    let mut vaults = Vec::new();
    for _ in 0..100 {
        vaults.push(create_vault(&app, &account).await);
    }
    let wrapped = account
        .keyset
        .wrap_vault_key(&SymmetricKey::generate().unwrap(), new_id().as_bytes())
        .unwrap();
    let (status, body) = call(
        &app,
        "POST",
        "/v1/vaults",
        Some(token),
        Some(json!({ "id": new_id(), "wrapped_key": b64(&wrapped.to_bytes()) })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::FORBIDDEN, error("limit_reached"))
    );

    // 49,999 items already, then the 50,000th is the last that fits.
    let vault = vaults[0];
    sqlx::query(
        "INSERT INTO items (vault_id, id, revision, seq, blob) \
         SELECT $1, gen_random_uuid(), 1, g, $2 FROM generate_series(1, 49999) g",
    )
    .bind(vault)
    .bind(vec![1u8; 41])
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE vaults SET seq = 49999 WHERE id = $1")
        .bind(vault)
        .execute(&db.pool)
        .await
        .unwrap();
    let last = new_id();
    assert_eq!(
        put_new(&app, token, &vault, &last, blob(41)).await.status,
        StatusCode::CREATED
    );
    let reply = put_new(&app, token, &vault, &new_id(), blob(41)).await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::FORBIDDEN, error("limit_reached"))
    );

    // Updating an existing item is still fine; a delete frees a place.
    let path = format!("/v1/vaults/{vault}/items/{last}");
    let reply = send(
        &app,
        "PUT",
        &path,
        Some(token),
        &[("if-match", "\"1\"")],
        Some(json!({ "revision": "2", "blob": blob(41) })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let reply = send(
        &app,
        "DELETE",
        &path,
        Some(token),
        &[("if-match", "\"2\"")],
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        put_new(&app, token, &vault, &new_id(), blob(41))
            .await
            .status,
        StatusCode::CREATED
    );

    // Other vaults are not affected.
    assert_eq!(
        put_new(&app, token, &vaults[1], &new_id(), blob(41))
            .await
            .status,
        StatusCode::CREATED
    );
    db.drop().await;
}

/// The app with the default limits, a clock the test moves, and `trusted`
/// proxies.
async fn limited(
    limits: Limits,
    trusted: &str,
) -> Option<(common::TestDb, Router, Arc<ManualClock>)> {
    let db = migrated_db().await?;
    let clock = Arc::new(ManualClock::default());
    let state = AppState::new(db.pool.clone(), ServerSecrets::generate().unwrap())
        .with_limits(limits, clock.clone())
        .with_trusted_proxies(TrustedProxies::parse(trusted).unwrap());
    Some((db, app(state), clock))
}

fn peer(ip: &str) -> Option<SocketAddr> {
    Some(SocketAddr::new(ip.parse().unwrap(), 40_000))
}

async fn post_from(app: &Router, ip: &str, path: &str, headers: &[(&str, &str)]) -> Reply {
    send_from(app, peer(ip), "POST", path, None, headers, Some(json!({}))).await
}

fn retry_after(reply: &Reply) -> u64 {
    reply.headers["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn unauthenticated_endpoints_are_limited_per_ip() {
    let Some((db, app, clock)) = limited(Limits::default(), "").await else {
        return;
    };
    // Registration: 10 requests (5 sign-ups) an hour.
    for _ in 0..10 {
        let reply = post_from(&app, "203.0.113.7", "/v1/auth/register/start", &[]).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    }
    let reply = post_from(&app, "203.0.113.7", "/v1/auth/register/finish", &[]).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(reply.body, error("rate_limited"));
    assert_eq!(retry_after(&reply), 360);
    // Another address is not affected; time refills the bucket.
    let reply = post_from(&app, "198.51.100.1", "/v1/auth/register/start", &[]).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    clock.advance(Duration::from_secs(360));
    let reply = post_from(&app, "203.0.113.7", "/v1/auth/register/start", &[]).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);

    // Pre-login and login share 30 a minute.
    for n in 0..30 {
        let path = if n % 2 == 0 {
            "/v1/auth/prelogin"
        } else {
            "/v1/auth/login/start"
        };
        assert_ne!(
            post_from(&app, "203.0.113.8", path, &[]).await.status,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    let reply = post_from(&app, "203.0.113.8", "/v1/auth/login/finish", &[]).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(retry_after(&reply), 2);
    // Proving the password to delete an account has 10 an hour of its own
    // (D74 §1): logins don't use them up.
    for n in 0..10 {
        let path = if n % 2 == 0 {
            "/v1/account/delete/start"
        } else {
            "/v1/account/delete/finish"
        };
        assert_eq!(
            post_from(&app, "203.0.113.8", path, &[]).await.status,
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }
    let reply = post_from(&app, "203.0.113.8", "/v1/account/delete/start", &[]).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(retry_after(&reply), 360);
    let reply = post_from(&app, "198.51.100.2", "/v1/account/delete/start", &[]).await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "another address");

    // Refresh: 10 a minute.
    for _ in 0..10 {
        let reply = post_from(&app, "203.0.113.9", "/v1/auth/refresh", &[]).await;
        assert_ne!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    }
    let reply = post_from(&app, "203.0.113.9", "/v1/auth/refresh", &[]).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(retry_after(&reply), 6);

    // IPv6 addresses count per /64.
    for n in 0..10 {
        let ip = format!("2001:db8:1:2::{n:x}");
        assert_ne!(
            post_from(&app, &ip, "/v1/auth/refresh", &[]).await.status,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    let reply = post_from(&app, "2001:db8:1:2:ffff::1", "/v1/auth/refresh", &[]).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    let reply = post_from(&app, "2001:db8:1:3::1", "/v1/auth/refresh", &[]).await;
    assert_ne!(reply.status, StatusCode::TOO_MANY_REQUESTS);

    // Health checks are never limited.
    for _ in 0..50 {
        let reply = send_from(
            &app,
            peer("203.0.113.9"),
            "GET",
            "/healthz",
            None,
            &[],
            None,
        )
        .await;
        assert_eq!(reply.status, StatusCode::OK);
    }
    db.drop().await;
}

#[tokio::test]
async fn trusted_proxies_decide_the_client_ip() {
    let Some((db, app, _)) = limited(Limits::default(), "10.0.0.0/8, 192.0.2.1").await else {
        return;
    };
    let refresh = "/v1/auth/refresh";
    let via = |client: &'static str| [("x-forwarded-for", client)];

    // Behind a trusted proxy the last forwarded address is the client.
    for _ in 0..10 {
        post_from(&app, "10.1.2.3", refresh, &via("9.9.9.9, 1.2.3.4")).await;
    }
    let reply = post_from(&app, "10.9.9.9", refresh, &via("1.2.3.4")).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    let reply = post_from(&app, "192.0.2.1", refresh, &via("5.6.7.8")).await;
    assert_ne!(reply.status, StatusCode::TOO_MANY_REQUESTS);

    // An untrusted peer's header is ignored, whatever it says.
    for n in 0..10 {
        let forged = ["1.1.1.1", "2.2.2.2"][n % 2];
        let reply = post_from(&app, "8.8.8.8", refresh, &[("x-forwarded-for", forged)]).await;
        assert_ne!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    }
    let reply = post_from(&app, "8.8.8.8", refresh, &via("3.3.3.3")).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    // A trusted peer with a garbled header counts as itself.
    for _ in 0..10 {
        post_from(&app, "10.0.0.5", refresh, &via("not an ip")).await;
    }
    let reply = post_from(&app, "10.0.0.5", refresh, &[]).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);

    for bad in ["10.0.0.0/33", "x", "10.0.0.0/", "::1/129"] {
        assert!(TrustedProxies::parse(bad).is_err(), "{bad}");
    }
    db.drop().await;
}

#[tokio::test]
async fn authenticated_requests_are_limited_per_account() {
    let limits = Limits {
        account: Rate::new(3, Duration::from_secs(60)),
        ..Limits::unlimited()
    };
    let Some((db, app, clock)) = limited(limits, "").await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let bob = register(&app, "bob@example.com").await;
    for _ in 0..3 {
        let (status, _) = call(&app, "GET", "/v1/account", Some(alice.access()), None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let reply = send(&app, "GET", "/v1/vaults", Some(alice.access()), &[], None).await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(retry_after(&reply), 20);

    // Per account: a second device of Alice's is limited too, Bob is not.
    let (status, phone) = login(&app, &alice, &new_id()).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        "GET",
        "/v1/account",
        phone["access_token"].as_str(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let (status, _) = call(&app, "GET", "/v1/account", Some(bob.access()), None).await;
    assert_eq!(status, StatusCode::OK);

    clock.advance(Duration::from_secs(20));
    let (status, _) = call(&app, "GET", "/v1/account", Some(alice.access()), None).await;
    assert_eq!(status, StatusCode::OK);
    db.drop().await;
}

#[tokio::test]
async fn every_response_carries_a_request_id() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let first = send(&app, "GET", "/healthz", None, &[], None).await;
    let second = send(&app, "GET", "/nope", None, &[], None).await;
    let id = |r: &Reply| r.headers["x-request-id"].to_str().unwrap().to_owned();
    assert_eq!(id(&first).len(), 32);
    assert!(id(&first).bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(id(&first), id(&second));
    db.drop().await;
}
