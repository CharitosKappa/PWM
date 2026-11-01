//! Throwaway Postgres databases for integration tests (D25).
//!
//! Set `PWM_TEST_DATABASE_URL` to a server the tests may create databases on
//! (`just db` starts one). Without it the database tests print "skipped" and
//! pass, unless `PWM_REQUIRE_DB=1` (CI), which makes a missing database a
//! failure.
#![allow(dead_code)]

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
