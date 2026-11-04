//! `GET /v1/account`, `/v1/devices` (D26, D43) and this device's push token
//! (D50 §3).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use pwm_crypto::{Envelope, SEALED_DEVICE_NAME_LEN};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::api::{ApiError, ApiJson, ApiPath, ApiRouter, AppState, b64};
use crate::auth::Kdf;
use crate::session::Authenticated;

pub fn routes() -> ApiRouter {
    Router::new()
        .route("/v1/account", get(account))
        .route("/v1/devices", get(devices))
        .route(
            "/v1/devices/{id}",
            axum::routing::patch(rename_device).delete(revoke_device),
        )
        .route(
            "/v1/devices/self/push",
            axum::routing::put(set_push).delete(clear_push),
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PushToken {
    app: String,
    /// The APNs device token, as hex.
    token: String,
    environment: String,
}

/// Registers or replaces this device's push token. A token another device
/// held (a reinstall) moves here.
async fn set_push(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiJson(req): ApiJson<PushToken>,
) -> Result<StatusCode, ApiError> {
    if !["ios", "macos"].contains(&req.app.as_str())
        || !["sandbox", "production"].contains(&req.environment.as_str())
    {
        return Err(ApiError::InvalidRequest);
    }
    let token = hex_token(&req.token).ok_or(ApiError::InvalidRequest)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("DELETE FROM push_tokens WHERE token = $1 AND device_id <> $2")
        .bind(&token)
        .bind(auth.device_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO push_tokens (device_id, app, token, environment) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (device_id) DO UPDATE \
         SET app = $2, token = $3, environment = $4, updated_at = now()",
    )
    .bind(auth.device_id)
    .bind(&req.app)
    .bind(&token)
    .bind(&req.environment)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_push(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<StatusCode, ApiError> {
    sqlx::query("DELETE FROM push_tokens WHERE device_id = $1")
        .bind(auth.device_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 32 to 100 bytes as hex, which the table also checks.
fn hex_token(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !(64..=200).contains(&text.len()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
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
    sqlx::query("DELETE FROM push_tokens WHERE device_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
    #[serde(with = "b64")]
    name_sealed: Vec<u8>,
}

/// Renames one of the account's devices (D43 §6). The name arrives sealed
/// under the account's key and padded, so the server stores only an
/// envelope of one fixed size. A revoked device keeps its last name.
async fn rename_device(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(req): ApiJson<Rename>,
) -> Result<StatusCode, ApiError> {
    if req.name_sealed.len() != SEALED_DEVICE_NAME_LEN
        || Envelope::from_bytes(&req.name_sealed).is_err()
    {
        return Err(ApiError::InvalidRequest);
    }
    let row = sqlx::query(
        "UPDATE devices SET name_sealed = CASE WHEN revoked_at IS NULL THEN $3 ELSE name_sealed END          WHERE id = $1 AND account_id = $2 RETURNING revoked_at IS NOT NULL",
    )
    .bind(id)
    .bind(auth.account_id)
    .bind(&req.name_sealed)
    .fetch_optional(&state.pool)
    .await?;
    match row.map(|r| r.get::<bool, _>(0)) {
        None => Err(ApiError::NotFound),
        Some(true) => Err(ApiError::Conflict("device_revoked")),
        Some(false) => Ok(StatusCode::NO_CONTENT),
    }
}
