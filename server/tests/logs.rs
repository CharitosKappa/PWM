//! No token, email or OPAQUE message reaches the log (D26). Its own test
//! binary with a process-wide subscriber: `tracing` caches per-callsite
//! interest globally, so a thread-local capture next to parallel tests can
//! miss events.

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use common::call;
use common::flows::*;
use serde_json::json;

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

/// With every log level on, no token, email or OPAQUE message reaches the
/// log, including on the refresh-reuse path that logs a warning. The only
/// test in this binary, so the global subscriber sees everything.
#[tokio::test]
async fn logs_hold_no_tokens_or_protocol_messages() {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let writer = Capture(buffer.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
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
    let _ = refresh(&app, tokens["refresh_token"].as_str().unwrap()).await;

    let log = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
    assert!(log.contains("session ended"), "the reuse warning is logged");
    for secret in [
        "pwma_",
        "pwmr_",
        "secret-person",
        &b64(&finalization),
        &b64(&response),
        &b64(&account.sealed_keyset),
        renewed["access_token"].as_str().unwrap(),
    ] {
        assert!(!log.contains(secret), "log contains {secret}");
    }
    db.drop().await;
}
