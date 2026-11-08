//! Throwaway Postgres databases for integration tests (D25).
//!
//! Set `PWM_TEST_DATABASE_URL` to a server the tests may create databases on
//! (`just db` starts one). Without it the database tests print "skipped" and
//! pass, unless `PWM_REQUIRE_DB=1` (CI), which makes a missing database a
//! failure.
#![allow(dead_code)]

pub mod driver;
pub mod flows;

use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, PgPool};

pub struct TestDb {
    pub pool: PgPool,
    admin: PgPool,
    name: String,
}

/// A fresh, empty database, or `None` (skip) when no test server is set.
pub async fn test_db() -> Option<TestDb> {
    let Ok(url) = std::env::var("PWM_TEST_DATABASE_URL") else {
        assert!(
            std::env::var("PWM_REQUIRE_DB").as_deref() != Ok("1"),
            "PWM_REQUIRE_DB=1 but PWM_TEST_DATABASE_URL is not set"
        );
        eprintln!("skipped: set PWM_TEST_DATABASE_URL to run database tests");
        return None;
    };
    let options = PgConnectOptions::from_str(&url).expect("PWM_TEST_DATABASE_URL is not a URL");
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .expect("cannot reach the test database server");
    let name = format!("pwm_test_{}", unique_suffix());
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .expect("CREATE DATABASE");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options.database(&name))
        .await
        .expect("connect to the test database");
    Some(TestDb { pool, admin, name })
}

/// A fresh database with every migration applied.
pub async fn migrated_db() -> Option<TestDb> {
    let db = test_db().await?;
    pwm_server::migrate(&db.pool).await.expect("migrate");
    Some(db)
}

impl TestDb {
    /// Drops the database. A test that panics leaves its database behind,
    /// which is harmless in the disposable test server.
    pub async fn drop(self) {
        self.pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.name
        )))
        .execute(&self.admin)
        .await
        .expect("DROP DATABASE");
    }
}

fn unique_suffix() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{}_{}_{}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// A response from [`send`].
pub struct Reply {
    pub status: axum::http::StatusCode,
    pub headers: axum::http::HeaderMap,
    /// The JSON body, `Value::Null` when empty.
    pub body: serde_json::Value,
}

/// Sends one request to the app; returns the status and the JSON body
/// (`Value::Null` when empty).
pub async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<serde_json::Value>,
) -> (axum::http::StatusCode, serde_json::Value) {
    let reply = send(app, method, path, bearer, &[], body).await;
    (reply.status, reply.body)
}

/// Like [`call`], with extra request headers, returning the response headers too.
pub async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<serde_json::Value>,
) -> Reply {
    send_from(app, None, method, path, bearer, headers, body).await
}

/// Like [`send`], as if the connection came from `peer` (what
/// `into_make_service_with_connect_info` provides in production).
pub async fn send_from(
    app: &axum::Router,
    peer: Option<std::net::SocketAddr>,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<serde_json::Value>,
) -> Reply {
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let mut request = axum::http::Request::builder().method(method).uri(path);
    if let Some(peer) = peer {
        request = request.extension(axum::extract::ConnectInfo(peer));
    }
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let request = match body {
        Some(json) => request
            .header("content-type", "application/json")
            .body(axum::body::Body::from(json.to_string())),
        None => request.body(axum::body::Body::empty()),
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
        })
    };
    Reply {
        status,
        headers,
        body,
    }
}
