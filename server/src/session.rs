//! Sessions (D26): one per device, tokens stored only as SHA-256 hashes.

use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use pwm_crypto::{Secret, TokenKind, generate_token, token_hash};
use serde::Serialize;
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::api::{ApiError, AppState};

pub const ACCESS_TTL_SECS: i64 = 15 * 60;
pub const REFRESH_TTL_SECS: i64 = 90 * 24 * 60 * 60;

/// The token pair returned by register, login and refresh.
#[derive(Serialize)]
pub struct Tokens {
    access_token: String,
    refresh_token: String,
    access_expires_in: i64,
    refresh_expires_in: i64,
}

struct NewTokens {
    access: Secret<String>,
    refresh: Secret<String>,
    access_hash: [u8; 32],
    refresh_hash: [u8; 32],
}

impl NewTokens {
    fn generate() -> Result<Self, ApiError> {
        let rng = |_| ApiError::Internal;
        let access = generate_token(TokenKind::Access).map_err(rng)?;
        let refresh = generate_token(TokenKind::Refresh).map_err(rng)?;
        let access_hash =
            token_hash(TokenKind::Access, access.expose_secret()).ok_or(ApiError::Internal)?;
        let refresh_hash =
            token_hash(TokenKind::Refresh, refresh.expose_secret()).ok_or(ApiError::Internal)?;
        Ok(Self {
            access,
            refresh,
            access_hash,
            refresh_hash,
        })
    }

    fn into_response(self) -> Tokens {
        Tokens {
            access_token: self.access.expose_secret().clone(),
            refresh_token: self.refresh.expose_secret().clone(),
            access_expires_in: ACCESS_TTL_SECS,
            refresh_expires_in: REFRESH_TTL_SECS,
        }
    }
}

fn secs(n: i64) -> f64 {
    n as f64
}

/// Starts a session for `device_id`, replacing any session it had.
pub async fn start_session(
    conn: &mut PgConnection,
    account_id: Uuid,
    device_id: Uuid,
) -> Result<Tokens, ApiError> {
    let tokens = NewTokens::generate()?;
    sqlx::query("DELETE FROM sessions WHERE device_id = $1")
        .bind(device_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO sessions (account_id, device_id, access_hash, access_expires_at, \
                               refresh_hash, refresh_expires_at) \
         VALUES ($1, $2, $3, now() + make_interval(secs => $4), \
                 $5, now() + make_interval(secs => $6))",
    )
    .bind(account_id)
    .bind(device_id)
    .bind(&tokens.access_hash[..])
    .bind(secs(ACCESS_TTL_SECS))
    .bind(&tokens.refresh_hash[..])
    .bind(secs(REFRESH_TTL_SECS))
    .execute(&mut *conn)
    .await?;
    Ok(tokens.into_response())
}

/// Exchanges a refresh token for a new pair. A refresh token that was
/// already replaced ends its session: it was probably copied.
pub async fn refresh_session(state: &AppState, refresh_token: &str) -> Result<Tokens, ApiError> {
    let hash = token_hash(TokenKind::Refresh, refresh_token).ok_or(ApiError::Unauthorized)?;
    let tokens = NewTokens::generate()?;
    let mut tx = state.pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE sessions s SET \
             access_hash = $2, access_expires_at = now() + make_interval(secs => $3), \
             prev_refresh_hash = s.refresh_hash, \
             refresh_hash = $4, refresh_expires_at = now() + make_interval(secs => $5) \
         FROM devices d \
         WHERE s.refresh_hash = $1 AND s.refresh_expires_at > now() \
           AND d.id = s.device_id AND d.revoked_at IS NULL \
         RETURNING s.device_id",
    )
    .bind(&hash[..])
    .bind(&tokens.access_hash[..])
    .bind(secs(ACCESS_TTL_SECS))
    .bind(&tokens.refresh_hash[..])
    .bind(secs(REFRESH_TTL_SECS))
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = updated else {
        let revoked = sqlx::query(
            "SELECT 1 FROM sessions s JOIN devices d ON d.id = s.device_id \
             WHERE s.refresh_hash = $1 AND d.revoked_at IS NOT NULL",
        )
        .bind(&hash[..])
        .fetch_optional(&mut *tx)
        .await?;
        if revoked.is_some() {
            return Err(ApiError::DeviceRevoked);
        }
        let ended = sqlx::query("DELETE FROM sessions WHERE prev_refresh_hash = $1 RETURNING id")
            .bind(&hash[..])
            .fetch_optional(&mut *tx)
            .await?;
        tx.commit().await?;
        if let Some(row) = ended {
            let session: Uuid = row.get(0);
            tracing::warn!(%session, "replaced refresh token presented again; session ended");
        }
        return Err(ApiError::Unauthorized);
    };
    let device_id: Uuid = row.get(0);
    touch_device(&mut tx, device_id).await?;
    tx.commit().await?;
    Ok(tokens.into_response())
}

pub async fn touch_device(conn: &mut PgConnection, device_id: Uuid) -> Result<(), ApiError> {
    sqlx::query("UPDATE devices SET last_seen_at = now() WHERE id = $1")
        .bind(device_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// A request with a valid access token from a device that is not revoked.
pub struct Authenticated {
    pub session_id: Uuid,
    pub account_id: Uuid,
    pub device_id: Uuid,
}

impl FromRequestParts<AppState> for Authenticated {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(ApiError::Unauthorized)?;
        let hash = token_hash(TokenKind::Access, token).ok_or(ApiError::Unauthorized)?;
        let row = sqlx::query(
            "SELECT s.id, s.account_id, s.device_id, d.revoked_at IS NOT NULL FROM sessions s \
             JOIN devices d ON d.id = s.device_id \
             WHERE s.access_hash = $1 AND s.access_expires_at > now()",
        )
        .bind(&hash[..])
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::Unauthorized)?;
        // A revoked device is told so, and its app clears its cache (D28).
        if row.get::<bool, _>(3) {
            return Err(ApiError::DeviceRevoked);
        }
        Ok(Self {
            session_id: row.get(0),
            account_id: row.get(1),
            device_id: row.get(2),
        })
    }
}
