//! Scheduled clean-up (D70): expired rows go, and tombstones older than the
//! horizon are compacted. It runs inside the server, at start-up and then
//! every [`MAINTENANCE_EVERY`]. Every delete is safe to repeat, so several
//! servers on one database need no coordination.

use std::time::Duration;

use sqlx::PgPool;

use crate::ServerError;
use crate::reports::REPORT_RETENTION_DAYS;

/// How often the clean-up runs.
pub const MAINTENANCE_EVERY: Duration = Duration::from_secs(60 * 60);

/// Tombstones are kept this long after the delete (D70 §2). A device whose
/// cursor is older pulls its vaults again from 0.
pub const TOMBSTONE_HORIZON_DAYS: i64 = 180;

/// Tombstones deleted per statement, so no statement runs long.
const COMPACT_BATCH: i64 = 10_000;

/// What one clean-up removed: counts only, never ids.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CleanUp {
    pub sessions: u64,
    pub login_attempts: u64,
    pub approvals: u64,
    pub reports: u64,
    pub tombstones: u64,
}

/// Runs the clean-up once.
pub async fn clean_up(pool: &PgPool) -> Result<CleanUp, ServerError> {
    // Includes revoked devices' sessions. They stay until then so the
    // device's refresh is told `device_revoked` and clears its cache (D28).
    let sessions = sqlx::query("DELETE FROM sessions WHERE refresh_expires_at <= now()")
        .execute(pool)
        .await?
        .rows_affected();
    let login_attempts = sqlx::query("DELETE FROM login_attempts WHERE expires_at <= now()")
        .execute(pool)
        .await?
        .rows_affected();
    let approvals = sqlx::query("DELETE FROM approvals WHERE expires_at <= now()")
        .execute(pool)
        .await?
        .rows_affected();
    let reports = sqlx::query(
        "DELETE FROM autofill_reports WHERE received_at < now() - make_interval(days => $1::int)",
    )
    .bind(REPORT_RETENTION_DAYS)
    .execute(pool)
    .await?
    .rows_affected();
    let mut tombstones = 0;
    loop {
        let gone = compact(pool).await?;
        tombstones += gone;
        if gone < COMPACT_BATCH as u64 {
            break;
        }
    }
    Ok(CleanUp {
        sessions,
        login_attempts,
        approvals,
        reports,
        tombstones,
    })
}

/// Deletes one batch of tombstones past the horizon and raises each
/// vault's `purged_seq` to the highest `seq` deleted, in one statement:
/// a delta sync (one snapshot) sees both or neither. Returns how many went.
async fn compact(pool: &PgPool) -> Result<u64, ServerError> {
    // Postgres runs every data-changing WITH part to completion, read or not.
    let gone: i64 = sqlx::query_scalar(
        "WITH gone AS ( \
             DELETE FROM tombstones WHERE (vault_id, item_id) IN ( \
                 SELECT vault_id, item_id FROM tombstones \
                 WHERE deleted_at < now() - make_interval(days => $1::int) LIMIT $2) \
             RETURNING vault_id, seq), \
         top AS (SELECT vault_id, max(seq) AS seq FROM gone GROUP BY vault_id), \
         raised AS ( \
             UPDATE vaults v SET purged_seq = greatest(v.purged_seq, top.seq) \
             FROM top WHERE v.id = top.vault_id) \
         SELECT count(*) FROM gone",
    )
    .bind(TOMBSTONE_HORIZON_DAYS)
    .bind(COMPACT_BATCH)
    .fetch_one(pool)
    .await?;
    Ok(gone as u64)
}

/// Runs [`clean_up`] now and then every [`MAINTENANCE_EVERY`] until the
/// task is aborted. A failed run is logged and retried at the next tick.
pub fn spawn_maintenance(pool: PgPool) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(MAINTENANCE_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match clean_up(&pool).await {
                Ok(done) => tracing::info!(
                    sessions = done.sessions,
                    login_attempts = done.login_attempts,
                    approvals = done.approvals,
                    reports = done.reports,
                    tombstones = done.tombstones,
                    "clean-up done"
                ),
                Err(err) => tracing::warn!("clean-up failed: {err}"),
            }
        }
    })
}
