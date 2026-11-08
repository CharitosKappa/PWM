//! `/v1/auth/*` (D26): pre-login, OPAQUE registration and login, refresh
//! and logout.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use pwm_crypto::{Envelope, KdfParams, opaque_record};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::api::{ApiError, ApiJson, ApiRouter, AppState, b64, is_unique_violation};
use crate::email::normalize_email;
use crate::session::{Authenticated, Tokens, refresh_session, start_session, touch_device};

const LOGIN_ATTEMPT_TTL_SECS: f64 = 60.0;

pub fn routes() -> ApiRouter {
    Router::new()
        .route("/v1/auth/prelogin", post(prelogin))
        .route("/v1/auth/register/start", post(register_start))
        .route("/v1/auth/register/finish", post(register_finish))
        .route("/v1/auth/login/start", post(login_start))
        .route("/v1/auth/login/finish", post(login_finish))
        .route("/v1/auth/refresh", post(refresh))
        .route("/v1/auth/logout", post(logout))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kdf {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl Kdf {
    fn params(self) -> KdfParams {
        KdfParams {
            memory_kib: self.memory_kib,
            iterations: self.iterations,
            parallelism: self.parallelism,
        }
    }

    /// From database columns (`integer`, checked positive by the schema).
    pub fn from_columns(m: i32, t: i32, p: i32) -> Result<Self, ApiError> {
        let n = |v: i32| u32::try_from(v).map_err(|_| ApiError::Internal);
        Ok(Self {
            memory_kib: n(m)?,
            iterations: n(t)?,
            parallelism: n(p)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Ios,
    Android,
    Macos,
    Windows,
    Linux,
    Extension,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Extension => "extension",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceInfo {
    id: Uuid,
    platform: Platform,
    #[serde(default, with = "b64::option")]
    name_sealed: Option<Vec<u8>>,
}

impl DeviceInfo {
    fn validate(&self) -> Result<(), ApiError> {
        match &self.name_sealed {
            Some(name) => envelope(name).map(|_| ()),
            None => Ok(()),
        }
    }
}

fn envelope(bytes: &[u8]) -> Result<Envelope, ApiError> {
    Envelope::from_bytes(bytes).map_err(|_| ApiError::InvalidRequest)
}

fn email(text: &str) -> Result<String, ApiError> {
    normalize_email(text).ok_or(ApiError::InvalidRequest)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreloginRequest {
    email: String,
}

#[derive(Serialize)]
struct PreloginResponse {
    account_id: Uuid,
    #[serde(with = "b64")]
    salt: Vec<u8>,
    kdf: Kdf,
}

/// The account's id, salt and KDF params; for an unknown email, stable fake
/// values with the default params (D12).
async fn prelogin(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<PreloginRequest>,
) -> Result<Json<PreloginResponse>, ApiError> {
    let email = email(&req.email)?;
    let row = sqlx::query("SELECT id, salt_pw, kdf_m, kdf_t, kdf_p FROM accounts WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await?;
    let response = match row {
        Some(row) => PreloginResponse {
            account_id: row.get(0),
            salt: row.get(1),
            kdf: Kdf::from_columns(row.get(2), row.get(3), row.get(4))?,
        },
        None => {
            let (id, salt) = state.secrets.prelogin_fake(&email);
            let d = KdfParams::DEFAULT;
            PreloginResponse {
                account_id: Uuid::from_bytes(id),
                salt: salt.to_vec(),
                kdf: Kdf {
                    memory_kib: d.memory_kib,
                    iterations: d.iterations,
                    parallelism: d.parallelism,
                },
            }
        }
    };
    Ok(Json(response))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterStartRequest {
    account_id: Uuid,
    #[serde(with = "b64")]
    registration_request: Vec<u8>,
}

#[derive(Serialize)]
struct RegisterStartResponse {
    #[serde(with = "b64")]
    registration_response: Vec<u8>,
}

async fn register_start(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<RegisterStartRequest>,
) -> Result<Json<RegisterStartResponse>, ApiError> {
    let registration_response = state
        .secrets
        .register_respond(req.account_id.as_bytes(), &req.registration_request)
        .map_err(|_| ApiError::InvalidRequest)?;
    Ok(Json(RegisterStartResponse {
        registration_response,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterFinishRequest {
    email: String,
    account_id: Uuid,
    #[serde(with = "b64")]
    registration_upload: Vec<u8>,
    #[serde(with = "b64")]
    salt: Vec<u8>,
    kdf: Kdf,
    #[serde(with = "b64")]
    sealed_keyset: Vec<u8>,
    #[serde(with = "b64")]
    x25519_pub: Vec<u8>,
    #[serde(with = "b64")]
    ed25519_pub: Vec<u8>,
    device: DeviceInfo,
}

/// Creates the account, its first device and a session in one transaction.
async fn register_finish(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<RegisterFinishRequest>,
) -> Result<(StatusCode, Json<Tokens>), ApiError> {
    let email = email(&req.email)?;
    let params = req.kdf.params();
    params.validate().map_err(|_| ApiError::InvalidRequest)?;
    let to_i32 = |v: u32| i32::try_from(v).map_err(|_| ApiError::InvalidRequest);
    if req.salt.len() != 16 || req.x25519_pub.len() != 32 || req.ed25519_pub.len() != 32 {
        return Err(ApiError::InvalidRequest);
    }
    envelope(&req.sealed_keyset)?;
    req.device.validate()?;
    let record = opaque_record(&req.registration_upload).map_err(|_| ApiError::InvalidRequest)?;

    let mut tx = state.pool.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO accounts (id, email, kdf_m, kdf_t, kdf_p, salt_pw, sealed_keyset, \
                               x25519_pub, ed25519_pub, opaque_record) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(req.account_id)
    .bind(&email)
    .bind(to_i32(params.memory_kib)?)
    .bind(to_i32(params.iterations)?)
    .bind(to_i32(params.parallelism)?)
    .bind(&req.salt)
    .bind(&req.sealed_keyset)
    .bind(&req.x25519_pub)
    .bind(&req.ed25519_pub)
    .bind(&record)
    .execute(&mut *tx)
    .await;
    match inserted {
        Err(err) if is_unique_violation(&err) => return Err(ApiError::Conflict("account_exists")),
        other => other?,
    };
    insert_device(&mut tx, req.account_id, &req.device).await?;
    let tokens = start_session(&mut tx, req.account_id, req.device.id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(tokens)))
}

async fn insert_device(
    conn: &mut PgConnection,
    account_id: Uuid,
    device: &DeviceInfo,
) -> Result<(), ApiError> {
    let inserted = sqlx::query(
        "INSERT INTO devices (id, account_id, platform, name_sealed) VALUES ($1, $2, $3, $4)",
    )
    .bind(device.id)
    .bind(account_id)
    .bind(device.platform.as_str())
    .bind(device.name_sealed.as_deref())
    .execute(conn)
    .await;
    match inserted {
        Err(err) if is_unique_violation(&err) => Err(ApiError::Conflict("device_conflict")),
        other => other.map(|_| ()).map_err(ApiError::from),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginStartRequest {
    account_id: Uuid,
    #[serde(with = "b64")]
    credential_request: Vec<u8>,
}

#[derive(Serialize)]
struct LoginStartResponse {
    login_id: Uuid,
    #[serde(with = "b64")]
    credential_response: Vec<u8>,
}

/// Answers the OPAQUE login request (a fake answer for an unknown account)
/// and keeps the server state for 60 seconds.
async fn login_start(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<LoginStartRequest>,
) -> Result<Json<LoginStartResponse>, ApiError> {
    let (login_id, credential_response) =
        begin_login(&state, req.account_id, &req.credential_request).await?;
    Ok(Json(LoginStartResponse {
        login_id,
        credential_response,
    }))
}

/// The first OPAQUE step for `account_id`, also used to prove the master
/// password before deleting an account (D74 §1): the attempt's id and the
/// credential response. The state is kept 60 seconds.
pub(crate) async fn begin_login(
    state: &AppState,
    account_id: Uuid,
    credential_request: &[u8],
) -> Result<(Uuid, Vec<u8>), ApiError> {
    let record: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT opaque_record FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_optional(&state.pool)
            .await?;
    let (login_state, credential_response) = state
        .secrets
        .login_respond(record.as_deref(), account_id.as_bytes(), credential_request)
        .map_err(|_| ApiError::InvalidRequest)?;
    sqlx::query("DELETE FROM login_attempts WHERE expires_at <= now()")
        .execute(&state.pool)
        .await?;
    let login_id: Uuid = sqlx::query_scalar(
        "INSERT INTO login_attempts (account_id, state, expires_at) \
         VALUES ($1, $2, now() + make_interval(secs => $3)) RETURNING id",
    )
    .bind(account_id)
    .bind(&login_state)
    .bind(LOGIN_ATTEMPT_TTL_SECS)
    .fetch_one(&state.pool)
    .await?;
    Ok((login_id, credential_response))
}

/// The last OPAQUE step: the account whose password was proven. The attempt
/// is used up whether it succeeds or not, and with `only` set, an attempt
/// for another account fails too. Every failure is `invalid_credentials`.
pub(crate) async fn verify_login(
    state: &AppState,
    login_id: Uuid,
    credential_finalization: &[u8],
    only: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    let attempt = sqlx::query(
        "DELETE FROM login_attempts WHERE id = $1 AND expires_at > now() \
         RETURNING account_id, state",
    )
    .bind(login_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::InvalidCredentials)?;
    let account_id: Uuid = attempt.get(0);
    let login_state: Vec<u8> = attempt.get(1);
    if only.is_some_and(|only| only != account_id) {
        return Err(ApiError::InvalidCredentials);
    }
    state
        .secrets
        .login_finish(&login_state, credential_finalization, account_id.as_bytes())
        .map_err(|_| ApiError::InvalidCredentials)?;
    Ok(account_id)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginFinishRequest {
    login_id: Uuid,
    #[serde(with = "b64")]
    credential_finalization: Vec<u8>,
    device: DeviceInfo,
}

/// Verifies the login, registers or refreshes the device and starts its
/// session. Every login failure answers `invalid_credentials`.
async fn login_finish(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<LoginFinishRequest>,
) -> Result<Json<Tokens>, ApiError> {
    req.device.validate()?;
    // Single use: the attempt is gone whether the login succeeds or not.
    let account_id = verify_login(&state, req.login_id, &req.credential_finalization, None).await?;

    let mut tx = state.pool.begin().await?;
    let existing = sqlx::query(
        "SELECT account_id, revoked_at IS NOT NULL FROM devices WHERE id = $1 FOR UPDATE",
    )
    .bind(req.device.id)
    .fetch_optional(&mut *tx)
    .await?;
    match existing {
        None => insert_device(&mut tx, account_id, &req.device).await?,
        Some(row) => {
            let (owner, revoked): (Uuid, bool) = (row.get(0), row.get(1));
            if owner != account_id {
                return Err(ApiError::Conflict("device_conflict"));
            }
            if revoked {
                return Err(ApiError::DeviceRevoked);
            }
            if let Some(name) = &req.device.name_sealed {
                sqlx::query("UPDATE devices SET name_sealed = $2 WHERE id = $1")
                    .bind(req.device.id)
                    .bind(name)
                    .execute(&mut *tx)
                    .await?;
            }
            touch_device(&mut tx, req.device.id).await?;
        }
    }
    let tokens = start_session(&mut tx, account_id, req.device.id).await?;
    tx.commit().await?;
    Ok(Json(tokens))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RefreshRequest {
    refresh_token: String,
}

async fn refresh(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<RefreshRequest>,
) -> Result<Json<Tokens>, ApiError> {
    Ok(Json(refresh_session(&state, &req.refresh_token).await?))
}

async fn logout(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(auth.session_id)
        .execute(&mut *tx)
        .await?;
    // A signed-out device gets no pushes (D50 §3).
    sqlx::query("DELETE FROM push_tokens WHERE device_id = $1")
        .bind(auth.device_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
