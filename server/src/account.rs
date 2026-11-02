//! `GET /v1/account` and `/v1/devices` (D26).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::api::{ApiError, ApiPath, ApiRouter, AppState, b64};
use crate::auth::Kdf;
use crate::session::Authenticated;

pub fn routes() -> ApiRouter {
    Router::new()
        .route("/v1/account", get(account))
        .route("/v1/devices", get(devices))
        .route("/v1/devices/{id}", delete(revoke_device))
}

#[derive(Serialize)]
struct AccountResponse {
    account_id: Uuid,
    email: String,
    kdf: Kdf,
    #[serde(with = "b64")]
    salt: Vec<u8>,
    #[serde(with = "b64")]
    sealed_keyset: Vec<u8>,
    #[serde(with = "b64")]
    x25519_pub: Vec<u8>,
    #[serde(with = "b64")]
    ed25519_pub: Vec<u8>,
    plan: String,
}

/// The sealed keyset and what unlocking needs, plus the plan: `free`
/// without a live subscription (a canceled one lasts to the end of the paid
/// period, `PRICING.md` §4).
async fn account(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<Json<AccountResponse>, ApiError> {
    let row = sqlx::query(
        "SELECT a.email, a.kdf_m, a.kdf_t, a.kdf_p, a.salt_pw, a.sealed_keyset, \
                a.x25519_pub, a.ed25519_pub, \
                CASE WHEN s.status IN ('active', 'past_due') \
                       OR (s.status = 'canceled' AND s.current_period_end > now()) \
                     THEN s.plan ELSE 'free' END \
         FROM accounts a LEFT JOIN subscriptions s ON s.account_id = a.id \
         WHERE a.id = $1",
    )
    .bind(auth.account_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(AccountResponse {
        account_id: auth.account_id,
        email: row.get(0),
        kdf: Kdf::from_columns(row.get(1), row.get(2), row.get(3))?,
        salt: row.get(4),
        sealed_keyset: row.get(5),
        x25519_pub: row.get(6),
        ed25519_pub: row.get(7),
        plan: row.get(8),
    }))
}

/// Times are milliseconds since the Unix epoch, as everywhere in the core.
#[derive(Serialize)]
struct Device {
    id: Uuid,
    platform: String,
    #[serde(with = "b64::option")]
    name_sealed: Option<Vec<u8>>,
    created_at: i64,
    last_seen_at: i64,
    revoked_at: Option<i64>,
    current: bool,
}

async fn devices(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<Json<Vec<Device>>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, platform, name_sealed, \
                (extract(epoch FROM created_at) * 1000)::bigint, \
                (extract(epoch FROM last_seen_at) * 1000)::bigint, \
                (extract(epoch FROM revoked_at) * 1000)::bigint \
         FROM devices WHERE account_id = $1 ORDER BY created_at, id",
    )
    .bind(auth.account_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let id: Uuid = row.get(0);
                Device {
                    id,
                    platform: row.get(1),
                    name_sealed: row.get(2),
                    created_at: row.get(3),
                    last_seen_at: row.get(4),
                    revoked_at: row.get(5),
                    current: id == auth.device_id,
                }
            })
            .collect(),
    ))
}

/// Revokes one of the account's devices. Its session stops working at once;
/// the row stays so the device is told `device_revoked` (and clears its
/// cache, D28) rather than a plain 401.
async fn revoke_device(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await?;
    let found = sqlx::query(
        "UPDATE devices SET revoked_at = coalesce(revoked_at, now()) \
         WHERE id = $1 AND account_id = $2 RETURNING id",
    )
    .bind(id)
    .bind(auth.account_id)
    .fetch_optional(&mut *tx)
    .await?;
    if found.is_none() {
        return Err(ApiError::NotFound);
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
