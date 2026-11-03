//! The device approval relay (D46 §4), end to end through the HTTP API
//! against a real Postgres: every step once and in order, one hand-out of
//! the envelope, expiry, the pending cap, and no account enumeration.

mod common;

use axum::http::StatusCode;
use common::flows::*;
use common::{call, send};
use pwm_crypto::{ApprovalKeys, approval_commitment};
use serde_json::{Value, json};

/// The new device's request; returns its id and poll token.
async fn ask(app: &axum::Router, email: &str, keys: &ApprovalKeys) -> (String, String) {
    let (status, body) = call(
        app,
        "POST",
        "/v1/approvals",
        None,
        Some(json!({ "email": email, "commitment": b64(&keys.commitment()), "platform": "ios" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["expires_in"], 600);
    (
        body["id"].as_str().unwrap().to_owned(),
        body["poll_token"].as_str().unwrap().to_owned(),
    )
}

/// A request with `Authorization: Approval <token>`.
async fn polled(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let reply = send(
        app,
        method,
        path,
        None,
        &[("authorization", &format!("Approval {token}"))],
        body,
    )
    .await;
    (reply.status, reply.body)
}

#[tokio::test]
async fn a_signed_in_device_approves_a_new_one() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let new = ApprovalKeys::generate().unwrap();
    let existing = ApprovalKeys::generate().unwrap();
    let (id, token) = ask(&app, "A@Example.com ", &new).await;
    let path = format!("/v1/approvals/{id}");

    // The signed-in device sees it, with the commitment but no key yet.
    let (status, list) = call(&app, "GET", "/v1/approvals", Some(account.access()), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let listed = &list.as_array().unwrap()[0];
    assert_eq!(listed["id"], json!(id));
    assert_eq!(listed["platform"], "ios");
    assert_eq!(listed["state"], "pending");
    assert_eq!(
        unb64(&listed["commitment"]),
        approval_commitment(&new.public())
    );
    assert!(listed["new_pub"].is_null());

    // Out of order: nothing to reveal or complete yet.
    assert_eq!(
        polled(
            &app,
            "POST",
            &format!("{path}/reveal"),
            &token,
            Some(json!({ "new_pub": b64(&new.public()) }))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{path}/complete"),
            Some(account.access()),
            Some(json!({ "envelope": b64(&[1u8; 60]) }))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );

    // E accepts; N sees E's key, then reveals its own.
    let (status, _) = call(
        &app,
        "POST",
        &format!("{path}/accept"),
        Some(account.access()),
        Some(json!({ "existing_pub": b64(&existing.public()) })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, state) = polled(&app, "GET", &path, &token, None).await;
    assert_eq!(state["state"], "accepted");
    assert_eq!(unb64(&state["existing_pub"]), existing.public());

    // A key that is not the committed one is refused.
    let other = ApprovalKeys::generate().unwrap();
    assert_eq!(
        polled(
            &app,
            "POST",
            &format!("{path}/reveal"),
            &token,
            Some(json!({ "new_pub": b64(&other.public()) }))
        )
        .await,
        (
            StatusCode::BAD_REQUEST,
            json!({ "error": "invalid_request" })
        )
    );
    let (status, _) = polled(
        &app,
        "POST",
        &format!("{path}/reveal"),
        &token,
        Some(json!({ "new_pub": b64(&new.public()) })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", "/v1/approvals", Some(account.access()), None).await;
    assert_eq!(unb64(&list[0]["new_pub"]), new.public());

    // E sends the sealed Secret Key; N gets it once.
    let id_bytes: [u8; 16] = *uuid::Uuid::parse_str(&id).unwrap().as_bytes();
    let envelope = existing
        .channel(&id_bytes, &new.public(), &existing.public())
        .unwrap()
        .seal(b"A1-SECRET")
        .unwrap();
    let (status, _) = call(
        &app,
        "POST",
        &format!("{path}/complete"),
        Some(account.access()),
        Some(json!({ "envelope": b64(&envelope.to_bytes()) })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, done) = polled(&app, "GET", &path, &token, None).await;
    assert_eq!(done["state"], "completed");
    assert_eq!(unb64(&done["envelope"]), envelope.to_bytes());
    let (_, again) = polled(&app, "GET", &path, &token, None).await;
    assert_eq!(again["state"], "delivered");
    assert!(again["envelope"].is_null(), "handed out once");
    // Nothing of it is stored any longer.
    let left: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT envelope FROM approvals WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(left.is_none());
    let (_, list) = call(&app, "GET", "/v1/approvals", Some(account.access()), None).await;
    assert_eq!(list, json!([]), "no longer pending");
    db.drop().await;
}

#[tokio::test]
async fn requests_are_private_capped_and_end() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let other = register(&app, "b@example.com").await;
    let keys = ApprovalKeys::generate().unwrap();

    // An unknown email gets the same answer, and the request goes nowhere.
    let (unknown, _) = ask(&app, "nobody@example.com", &keys).await;
    assert!(!unknown.is_empty());

    // Another account's device, or a wrong poll token, sees nothing.
    let (id, token) = ask(&app, "a@example.com", &keys).await;
    let path = format!("/v1/approvals/{id}");
    let (_, list) = call(&app, "GET", "/v1/approvals", Some(other.access()), None).await;
    assert_eq!(list, json!([]));
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{path}/accept"),
            Some(other.access()),
            Some(json!({ "existing_pub": b64(&keys.public()) }))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let wrong = pwm_crypto::generate_token(pwm_crypto::TokenKind::Approval).unwrap();
    assert_eq!(
        polled(&app, "GET", &path, wrong.expose_secret(), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", &path, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );

    // At most 3 pending per account: a fourth looks the same but goes nowhere.
    ask(&app, "a@example.com", &keys).await;
    ask(&app, "a@example.com", &keys).await;
    ask(&app, "a@example.com", &keys).await;
    let (_, list) = call(&app, "GET", "/v1/approvals", Some(account.access()), None).await;
    assert_eq!(list.as_array().unwrap().len(), 3);

    // E denies one; N ends another; both are gone.
    let (status, _) = call(
        &app,
        "POST",
        &format!("{path}/deny"),
        Some(account.access()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, ended) = polled(&app, "GET", &path, &token, None).await;
    assert_eq!(ended["state"], "ended");
    let (id2, token2) = ask(&app, "a@example.com", &keys).await;
    let (status, _) = polled(
        &app,
        "DELETE",
        &format!("/v1/approvals/{id2}"),
        &token2,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // After 10 minutes a request is gone.
    sqlx::query("UPDATE approvals SET expires_at = now() - interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    let (_, list) = call(&app, "GET", "/v1/approvals", Some(account.access()), None).await;
    assert_eq!(list, json!([]));
    assert_eq!(
        polled(&app, "GET", &path, &token, None).await.0,
        StatusCode::NOT_FOUND
    );

    // Bad input is refused without echoing it.
    let invalid = (
        StatusCode::BAD_REQUEST,
        json!({ "error": "invalid_request" }),
    );
    for body in [
        json!({ "email": "a@example.com", "commitment": b64(&[1u8; 31]), "platform": "ios" }),
        json!({ "email": "a@example.com", "commitment": b64(&[1u8; 32]), "platform": "symbian" }),
        json!({ "email": "not an email", "commitment": b64(&[1u8; 32]), "platform": "ios" }),
        json!({ "email": "a@example.com", "commitment": b64(&[1u8; 32]), "platform": "ios", "x": 1 }),
    ] {
        assert_eq!(
            call(&app, "POST", "/v1/approvals", None, Some(body)).await,
            invalid
        );
    }
    db.drop().await;
}
