//! Logs hold no secrets, ids or contents (D26, D29). Its own test binary
//! with a process-wide subscriber: `tracing` caches per-callsite interest
//! globally, so a thread-local capture next to parallel tests can miss
//! events.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use common::flows::*;
use common::{call, send};
use pwm_crypto::SymmetricKey;
use pwm_server::LogFormat;
use serde_json::{Value, json};

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The server's own log pipeline (JSON lines through the scrubber) at
/// TRACE, over a full run: register, login, refresh and its reuse warning,
/// vault, item, sync. The only test in this binary, so the global
/// subscriber sees everything.
#[tokio::test]
async fn logs_hold_no_secrets_ids_or_contents() {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = Capture(buffer.clone());
    let subscriber = pwm_server::log_subscriber(LogFormat::Json, "trace", move || writer.clone());
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let Some((db, app)) = setup().await else {
        return;
    };

    let account = register(&app, "secret-person@example.com").await;
    let (client, login_id, response) = login_start(&app, &account).await;
    let finalization = client
        .finish(&account.auth_secret, account.id.as_bytes(), &response)
        .unwrap();
    let (_, tokens) = call(
        &app,
        "POST",
        "/v1/auth/login/finish",
        None,
        Some(
            json!({ "login_id": login_id, "credential_finalization": b64(&finalization),
                     "device": device_json(&account.id, &account.keyset, &account.device, "L") }),
        ),
    )
    .await;
    let (_, renewed) = refresh(&app, tokens["refresh_token"].as_str().unwrap()).await;
    let access = renewed["access_token"].as_str().unwrap();

    let vault = new_id();
    let key = SymmetricKey::generate().unwrap();
    let wrapped = account
        .keyset
        .wrap_vault_key(&key, vault.as_bytes())
        .unwrap();
    call(
        &app,
        "POST",
        "/v1/vaults",
        Some(access),
        Some(json!({ "id": vault, "wrapped_key": b64(&wrapped.to_bytes()) })),
    )
    .await;
    let item = new_id();
    let blob = b64(&pwm_crypto::seal(
        &key,
        b"item contents",
        &pwm_crypto::Aad::item(1, vault.as_bytes(), item.as_bytes(), 1),
    )
    .unwrap()
    .to_bytes());
    let reply = send(
        &app,
        "PUT",
        &format!("/v1/vaults/{vault}/items/{item}"),
        Some(access),
        &[("if-none-match", "*")],
        Some(json!({ "revision": "1", "blob": blob })),
    )
    .await;
    assert_eq!(reply.status, axum::http::StatusCode::CREATED);
    let request_id = reply.headers["x-request-id"].to_str().unwrap().to_owned();
    call(
        &app,
        "GET",
        &format!("/v1/vaults/{vault}/items?since=0"),
        Some(access),
        None,
    )
    .await;
    // Last, because it ends the session: a replaced refresh token again.
    let _ = refresh(&app, tokens["refresh_token"].as_str().unwrap()).await;

    // A careless log line is caught by the scrubber.
    tracing::info!("oops: {access} {}", "secret-person@example.com");

    let log = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(log.contains("session ended"), "the reuse warning is logged");
    assert!(
        log.contains("oops: [REDACTED] [REDACTED]"),
        "the scrubber redacts"
    );
    for secret in [
        "pwma_",
        "pwmr_",
        "secret-person",
        &b64(&finalization),
        &b64(&response),
        &b64(&account.sealed_keyset),
        &b64(&wrapped.to_bytes()),
        &blob,
        access,
        &account.id.to_string(),
        &account.device.to_string(),
        &vault.to_string(),
        &item.to_string(),
    ] {
        assert!(!log.contains(secret), "log contains {secret}");
    }

    // Every line is JSON; every request has one line with exactly these
    // fields, and the route template instead of the path.
    let lines: Vec<Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let requests: Vec<&Value> = lines
        .iter()
        .filter(|l| l["target"] == "pwm_server::request")
        .collect();
    assert!(requests.len() >= 9, "{} request lines", requests.len());
    for line in &requests {
        let mut fields: Vec<&str> = line["fields"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            [
                "duration_ms",
                "message",
                "method",
                "request_id",
                "route",
                "status"
            ]
        );
    }
    let put = requests
        .iter()
        .find(|l| l["fields"]["request_id"] == request_id.as_str())
        .unwrap();
    assert_eq!(
        put["fields"]["route"],
        "/v1/vaults/{vault_id}/items/{item_id}"
    );
    assert_eq!(put["fields"]["method"], "PUT");
    assert_eq!(put["fields"]["status"], 201);
    db.drop().await;
}
