use std::time::Duration;

use sqlx::PgPool;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;

use crate::ServerError;

/// Schema migrations (`server/migrations/`), embedded at build time.
pub static MIGRATOR: Migrator = sqlx::migrate!();

const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);

/// A connection pool. Connections are opened on first use, so the server
/// can start (and `/healthz` reports 503) while Postgres is unreachable.
pub fn connect(database_url: &str) -> Result<PgPool, ServerError> {
    Ok(PgPoolOptions::new()
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect_lazy(database_url)?)
}

/// Applies pending migrations under sqlx's advisory lock, so several
/// instances can start at once. Refuses a database with a migration this
/// build doesn't know (migrated by a newer server) or whose applied
/// migration was edited (checksum mismatch).
pub async fn migrate(pool: &PgPool) -> Result<(), ServerError> {
    Ok(MIGRATOR.run(pool).await?)
}
