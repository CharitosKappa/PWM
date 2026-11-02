//! Autofill failure reports (D38 §1, §6) through the HTTP API against a real
//! Postgres: anonymous, schema-checked, rate-limited per IP, kept 90 days.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{migrated_db, send_from};
use pwm_crypto::ServerSecrets;
use pwm_server::{
    AppState, Limits, MAX_REPORT_BYTES, ManualClock, Rate, app, check_report, export_reports,
};
use serde_json::{Value, json};

fn report() -> Value {
    json!({
        "v": 1,
        "site": "https://login.example.com",
        "problem": "nothing_filled",
        "matched": true,
        "software": {"extension": "0.1.0", "core": "0.0.0", "rules": 0, "browser": "Chrome 140"},
        "frames": [{
            "relation": "top",
            "fields": [{
                "form": 0, "tag": "input", "type": "email", "autocomplete": "username",
                "inputmode": "", "max_length": null, "visible": true, "name": "email",
                "id": "email", "label": "email", "placeholder": "", "aria_label": "",
                "class": "username", "confidence": 0.9, "fill": true,
            }],
        }],
    })
}

fn with(path: &[&str], value: Value) -> Value {
    let mut r = report();
    let mut at = &mut r;
    for key in &path[..path.len() - 1] {
        at = match key.parse::<usize>() {
            Ok(i) => &mut at[i],
            Err(_) => &mut at[*key],
        };
    }
    at[path[path.len() - 1]] = value;
    r
}

#[test]
fn the_schema_lets_only_structure_through() {
    assert_eq!(MAX_REPORT_BYTES, pwm_autofill_max());
    assert_eq!(
        check_report(&report()).unwrap(),
        "https://login.example.com"
    );
    for bad in [
        with(&["v"], json!(2)),
        with(&["site"], json!("https://login.example.com/signin?token=x")),
        with(&["site"], json!("chrome://settings")),
        with(&["site"], json!("https://user:pw@example.com")),
        with(&["problem"], json!("my own words")),
        with(&["matched"], json!("yes")),
        with(&["frames", "0", "relation"], json!("https://frame.example")),
        with(&["frames", "0", "fields", "0", "value"], json!("hunter2")),
        with(
            &["frames", "0", "fields", "0", "label"],
            json!("x".repeat(161)),
        ),
        with(&["frames", "0", "url"], json!("https://x/y")),
        with(&["software", "cookies"], json!("x")),
        with(&["password"], json!("hunter2")),
        json!([]),
    ] {
        assert!(check_report(&bad).is_err(), "{bad}");
    }
    let mut many = report();
    many["frames"] = json!(
        (0..21)
            .map(|_| json!({"relation": "top", "fields": []}))
            .collect::<Vec<_>>()
    );
    assert!(check_report(&many).is_err());
}

/// `pwm_autofill::MAX_REPORT_BYTES`, written here so the server does not
/// depend on the engine.
fn pwm_autofill_max() -> usize {
    32 * 1024
}

fn peer(n: u8) -> Option<SocketAddr> {
    Some(SocketAddr::from(([203, 0, 113, n], 4000)))
}

#[tokio::test]
async fn reports_are_stored_without_account_or_address_and_exported() {
    let Some(db) = migrated_db().await else {
        return;
    };
    let app = app(AppState::new(
        db.pool.clone(),
        ServerSecrets::generate().unwrap(),
    ));
    let reply = send_from(
        &app,
        peer(1),
        "POST",
        "/v1/reports/autofill",
        None,
        &[],
        Some(report()),
    )
    .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.body);

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_name = 'autofill_reports' ORDER BY ordinal_position",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(columns, ["id", "received_at", "site", "report"]);

    let lines = export_reports(&db.pool, None).await.unwrap();
    assert_eq!(lines.len(), 1);
    let line: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(line["site"], "https://login.example.com");
    assert_eq!(line["report"], report());
    assert!(line["received_at"].as_str().unwrap().ends_with('Z'));
    assert!(!lines[0].contains("203.0.113"));
    assert!(
        export_reports(&db.pool, Some("2999-01-01"))
            .await
            .unwrap()
            .is_empty()
    );

    // Refused reports and oversized bodies are not stored.
    let bad = with(&["frames", "0", "fields", "0", "value"], json!("hunter2"));
    let reply = send_from(
        &app,
        peer(1),
        "POST",
        "/v1/reports/autofill",
        None,
        &[],
        Some(bad),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    let huge = with(
        &["software", "extension"],
        json!("x".repeat(MAX_REPORT_BYTES)),
    );
    let reply = send_from(
        &app,
        peer(1),
        "POST",
        "/v1/reports/autofill",
        None,
        &[],
        Some(huge),
    )
    .await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(export_reports(&db.pool, None).await.unwrap().len(), 1);
    db.drop().await;
}

#[tokio::test]
async fn old_reports_are_deleted_on_the_next_insert() {
    let Some(db) = migrated_db().await else {
        return;
    };
    let app = app(AppState::new(
        db.pool.clone(),
        ServerSecrets::generate().unwrap(),
    ));
    sqlx::query(
        "INSERT INTO autofill_reports (received_at, site, report) \
         VALUES (now() - interval '91 days', 'https://old.example', '{}'::jsonb), \
                (now() - interval '89 days', 'https://recent.example', '{}'::jsonb)",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    let reply = send_from(
        &app,
        peer(1),
        "POST",
        "/v1/reports/autofill",
        None,
        &[],
        Some(report()),
    )
    .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    let sites: Vec<String> =
        sqlx::query_scalar("SELECT site FROM autofill_reports ORDER BY received_at")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert_eq!(
        sites,
        ["https://recent.example", "https://login.example.com"]
    );
    db.drop().await;
}

#[tokio::test]
async fn reports_are_rate_limited_per_address() {
    let Some(db) = migrated_db().await else {
        return;
    };
    let clock = Arc::new(ManualClock::default());
    let limits = Limits {
        report: Rate::new(2, Duration::from_secs(3600)),
        ..Limits::unlimited()
    };
    let app = app(
        AppState::new(db.pool.clone(), ServerSecrets::generate().unwrap())
            .with_limits(limits, clock.clone()),
    );
    let post = |n| {
        send_from(
            &app,
            peer(n),
            "POST",
            "/v1/reports/autofill",
            None,
            &[],
            Some(report()),
        )
    };
    assert_eq!(post(1).await.status, StatusCode::ACCEPTED);
    assert_eq!(post(1).await.status, StatusCode::ACCEPTED);
    let limited = post(1).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    // Another address is counted apart.
    assert_eq!(post(2).await.status, StatusCode::ACCEPTED);
    clock.advance(Duration::from_secs(1800));
    assert_eq!(post(1).await.status, StatusCode::ACCEPTED);
    db.drop().await;
}
