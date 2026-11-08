//! Account deletion (D74): the master password proven again with an OPAQUE
//! login made only for it, the subscriptions ended first, then everything
//! deleted at once. What stays is short-lived and names nobody: the refresh
//! token hashes that tell the other devices to wipe themselves, and the
//! deleted ids that keep a restored backup from bringing an account back,
//! in the database and in a file outside it.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::api::{ApiError, ApiJson, ApiRouter, AppState, b64};
use crate::auth::{begin_login, verify_login};
use crate::error::ServerError;
use crate::session::Authenticated;

/// How long a deleted account's id is kept, in the database and in the
/// file: twice the backup age `DEPLOYMENT.md` recommends (D74 §6).
pub const DELETED_ACCOUNTS_KEPT_DAYS: i64 = 60;

const DAY_SECS: i64 = 24 * 60 * 60;

pub fn routes() -> ApiRouter {
    Router::new()
        .route("/v1/account/delete/start", post(start))
        .route("/v1/account/delete/finish", post(finish))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRequest {
    #[serde(with = "b64")]
    credential_request: Vec<u8>,
}

#[derive(Serialize)]
struct StartResponse {
    login_id: Uuid,
    #[serde(with = "b64")]
    credential_response: Vec<u8>,
}

/// The first OPAQUE step, for the session's own account only (D74 §1).
async fn start(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiJson(req): ApiJson<StartRequest>,
) -> Result<Json<StartResponse>, ApiError> {
    let (login_id, credential_response) =
        begin_login(&state, auth.account_id, &req.credential_request).await?;
    Ok(Json(StartResponse {
        login_id,
        credential_response,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinishRequest {
    login_id: Uuid,
    #[serde(with = "b64")]
    credential_finalization: Vec<u8>,
}

/// The last OPAQUE step; with the right password, the subscriptions end and
/// the account is deleted in the same request (D74 §1, §4). A wrong password
/// changes nothing, and so does a provider that can't be reached.
///
/// A failed proof is 403 `invalid_credentials`, not 401: under a session,
/// 401 means the access token, and the client would refresh it and retry.
async fn finish(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiJson(req): ApiJson<FinishRequest>,
) -> Result<StatusCode, ApiError> {
    verify_login(
        &state,
        req.login_id,
        &req.credential_finalization,
        Some(auth.account_id),
    )
    .await
    .map_err(|err| match err {
        ApiError::InvalidCredentials => ApiError::Forbidden("invalid_credentials"),
        other => other,
    })?;
    crate::billing::end_for_deletion(&state, auth.account_id).await?;

    let mut tx = state.pool.begin().await?;
    // Read before the cascade takes them: the other devices' push tokens.
    let tokens = crate::push::account_tokens(&mut tx, auth.account_id, auth.device_id).await?;
    // Every session, this device's too: if the answer is lost, its retry is
    // told `account_deleted` as well.
    sqlx::query(
        "INSERT INTO deleted_sessions (refresh_hash, expires_at) \
         SELECT refresh_hash, refresh_expires_at FROM sessions \
         WHERE account_id = $1 AND refresh_expires_at > now() \
         ON CONFLICT DO NOTHING",
    )
    .bind(auth.account_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO deleted_accounts (account_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(auth.account_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(auth.account_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // After the commit, so a failed delete never lists a live account.
    state.deletions.record(auth.account_id).await;
    state.push.account_deleted(tokens);
    tracing::info!("an account was deleted");
    Ok(StatusCode::NO_CONTENT)
}

/// Removes what deletion left behind once it's no longer needed: refresh
/// token hashes past their expiry, and ids older than 60 days. Returns how
/// many rows went.
pub async fn purge_deletions(pool: &PgPool) -> Result<u64, ServerError> {
    let sessions = sqlx::query("DELETE FROM deleted_sessions WHERE expires_at <= now()")
        .execute(pool)
        .await?
        .rows_affected();
    let accounts = sqlx::query(
        "DELETE FROM deleted_accounts WHERE deleted_at <= now() - make_interval(days => $1)",
    )
    .bind(i32::try_from(DELETED_ACCOUNTS_KEPT_DAYS).unwrap_or(i32::MAX))
    .execute(pool)
    .await?
    .rows_affected();
    Ok(sessions + accounts)
}

/// After restoring a backup (D74 §6): deletes again every account the
/// database or the deletions file lists, and keeps them listed. Returns how
/// many accounts the backup had brought back.
pub async fn redelete(pool: &PgPool, log: &DeletionLog) -> Result<u64, ServerError> {
    let mut tx = pool.begin().await?;
    for (id, at) in log.read().await? {
        sqlx::query(
            "INSERT INTO deleted_accounts (account_id, deleted_at) \
             VALUES ($1, to_timestamp($2)) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(at as f64)
        .execute(&mut *tx)
        .await?;
    }
    let gone =
        sqlx::query("DELETE FROM accounts WHERE id IN (SELECT account_id FROM deleted_accounts)")
            .execute(&mut *tx)
            .await?
            .rows_affected();
    tx.commit().await?;
    Ok(gone)
}

/// Deleted accounts in a file outside the database (D74 §6), one line per
/// deletion: `<account id> <unix seconds>`. It lives on its own volume, so a
/// restored backup doesn't take it back in time. One server writes it.
#[derive(Clone, Default)]
pub struct DeletionLog {
    path: Option<Arc<PathBuf>>,
    /// Appending and pruning never overlap.
    lock: Arc<Mutex<()>>,
}

impl DeletionLog {
    pub fn off() -> Self {
        Self::default()
    }

    pub fn at(path: PathBuf) -> Self {
        Self {
            path: Some(Arc::new(path)),
            lock: Arc::default(),
        }
    }

    pub fn is_on(&self) -> bool {
        self.path.is_some()
    }

    /// Appends one deletion and syncs it to disk. A failure is logged and
    /// nothing more: the account is already deleted, and the table still
    /// lists it.
    pub async fn record(&self, account: Uuid) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let lock = self.lock.clone();
        let line = format!("{account} {}\n", unix_secs());
        let written = tokio::task::spawn_blocking(move || {
            let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.as_path())?;
            file.write_all(line.as_bytes())?;
            file.sync_data()
        })
        .await;
        if !matches!(written, Ok(Ok(()))) {
            tracing::warn!("deletions: the deletions file could not be written");
        }
    }

    /// The deletions of the last 60 days. A missing file is empty, and a line
    /// that doesn't parse (a write cut short) is skipped.
    pub async fn read(&self) -> Result<Vec<(Uuid, i64)>, ServerError> {
        let Some(path) = self.path.clone() else {
            return Ok(Vec::new());
        };
        let lock = self.lock.clone();
        let text = tokio::task::spawn_blocking(move || {
            let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
            match std::fs::read_to_string(path.as_path()) {
                Ok(text) => Ok(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                Err(e) => Err(e),
            }
        })
        .await
        .map_err(|_| ServerError::Config("the deletions file could not be read"))?
        .map_err(|_| ServerError::Config("the deletions file could not be read"))?;
        let since = unix_secs() - DELETED_ACCOUNTS_KEPT_DAYS * DAY_SECS;
        Ok(parse(&text).filter(|(_, at)| *at > since).collect())
    }

    /// Drops the lines older than 60 days, by writing the rest to a new file
    /// next to it and renaming it over the old one.
    pub async fn prune(&self) -> Result<(), ServerError> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        let lock = self.lock.clone();
        let since = unix_secs() - DELETED_ACCOUNTS_KEPT_DAYS * DAY_SECS;
        let done = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
            let text = match std::fs::read_to_string(path.as_path()) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(e),
            };
            let kept: String = parse(&text)
                .filter(|(_, at)| *at > since)
                .map(|(id, at)| format!("{id} {at}\n"))
                .collect();
            if kept == text {
                return Ok(());
            }
            let mut name = path.file_name().unwrap_or_default().to_owned();
            name.push(".new");
            let fresh = path.with_file_name(name);
            let mut file = std::fs::File::create(&fresh)?;
            file.write_all(kept.as_bytes())?;
            file.sync_data()?;
            std::fs::rename(&fresh, path.as_path())
        })
        .await;
        match done {
            Ok(Ok(())) => Ok(()),
            _ => Err(ServerError::Config(
                "the deletions file could not be pruned",
            )),
        }
    }

    /// Purges the tables and prunes the file a minute after start, then daily.
    pub fn start_daily(&self, pool: PgPool) {
        let log = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut every = tokio::time::interval(Duration::from_secs(DAY_SECS as u64));
            loop {
                every.tick().await;
                if purge_deletions(&pool).await.is_err() {
                    tracing::warn!("deletions: the daily purge could not reach the database");
                }
                if log.prune().await.is_err() {
                    tracing::warn!("deletions: the deletions file could not be pruned");
                }
            }
        });
    }
}

fn parse(text: &str) -> impl Iterator<Item = (Uuid, i64)> + '_ {
    text.lines().filter_map(|line| {
        let (id, at) = line.split_once(' ')?;
        Some((Uuid::parse_str(id).ok()?, at.parse().ok()?))
    })
}

fn unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_cut_short_are_skipped() {
        let id = Uuid::from_u128(7);
        let text = format!("{id} 100\nnot-a-line\n{id} \n{id}");
        assert_eq!(parse(&text).collect::<Vec<_>>(), [(id, 100)]);
    }
}
