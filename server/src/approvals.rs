//! The device approval relay (D46 §4). The new device ("N") is not signed
//! in: it makes a request with a commitment to its key and gets a poll
//! token, then polls with `Authorization: Approval <token>`. A signed-in
//! device ("E") of the account lists, accepts, completes or denies. The
//! server checks the order of steps and the commitment, and stores only
//! public keys, the commitment and the sealed Secret Key, which it hands
//! out once.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use pwm_crypto::{Envelope, TokenKind, check_approval_commitment, generate_token, token_hash};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::api::{ApiError, ApiJson, ApiPath, ApiRouter, AppState, b64};
use crate::email::normalize_email;
use crate::session::Authenticated;

/// How long a request lives.
const LIFETIME_SECS: i64 = 600;
/// Pending requests per account; more go nowhere (D46 §4).
const MAX_PENDING: i64 = 3;
const PLATFORMS: [&str; 6] = ["ios", "android", "macos", "windows", "linux", "extension"];

pub fn routes() -> ApiRouter {
    Router::new()
        .route("/v1/approvals", post(ask).get(list))
        .route("/v1/approvals/{id}", get(poll).delete(end))
        .route("/v1/approvals/{id}/reveal", post(reveal))
        .route("/v1/approvals/{id}/accept", post(accept))
        .route("/v1/approvals/{id}/complete", post(complete))
        .route("/v1/approvals/{id}/deny", post(deny))
}

fn bytes32(bytes: &[u8]) -> Result<[u8; 32], ApiError> {
    bytes.try_into().map_err(|_| ApiError::InvalidRequest)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ask {
    email: String,
    #[serde(with = "b64")]
    commitment: Vec<u8>,
    platform: String,
}

#[derive(Serialize)]
struct Asked {
    id: Uuid,
    poll_token: String,
    expires_in: i64,
}

/// `POST /v1/approvals` (N, unauthenticated, limited per IP). The same
/// answer whatever the email, so it tells nobody which emails have accounts.
async fn ask(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<Ask>,
) -> Result<(StatusCode, Json<Asked>), ApiError> {
    let commitment = bytes32(&req.commitment)?;
    if !PLATFORMS.contains(&req.platform.as_str()) {
        return Err(ApiError::InvalidRequest);
    }
    let email = normalize_email(&req.email).ok_or(ApiError::InvalidRequest)?;
    let token = generate_token(TokenKind::Approval).map_err(|_| ApiError::Internal)?;
    let hash = token_hash(TokenKind::Approval, token.expose_secret()).ok_or(ApiError::Internal)?;
    let mut tx = state.pool.begin().await?;
    // Housekeeping: requests past their lifetime go.
    sqlx::query("DELETE FROM approvals WHERE expires_at <= now()")
        .execute(&mut *tx)
        .await?;
    let account: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM accounts WHERE email = $1 FOR UPDATE")
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await?;
    let account = match account {
        Some(id) => {
            let pending: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM approvals WHERE account_id = $1 \
                 AND state IN ('pending', 'accepted', 'revealed') AND expires_at > now()",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            (pending < MAX_PENDING).then_some(id)
        }
        None => None,
    };
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO approvals (account_id, platform, commitment, poll_hash, expires_at) \
         VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5)) RETURNING id",
    )
    .bind(account)
    .bind(&req.platform)
    .bind(commitment.as_slice())
    .bind(hash.as_slice())
    .bind(LIFETIME_SECS as f64)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(Asked {
            id,
            poll_token: token.expose_secret().clone(),
            expires_in: LIFETIME_SECS,
        }),
    ))
}

/// The poll token's hash, from `Authorization: Approval <token>`.
fn poll_hash(headers: &HeaderMap) -> Result<[u8; 32], ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Approval "))
        .and_then(|token| token_hash(TokenKind::Approval, token))
        .ok_or(ApiError::Unauthorized)
}

/// `GET /v1/approvals/{id}` (N): the state and what N needs next. A
/// completed request hands out its envelope once and becomes `delivered`.
async fn poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let hash = poll_hash(&headers)?;
    let delivered = sqlx::query(
        "WITH done AS (SELECT id, envelope FROM approvals \
           WHERE id = $1 AND poll_hash = $2 AND state = 'completed' AND expires_at > now() FOR UPDATE) \
         UPDATE approvals a SET state = 'delivered', envelope = NULL FROM done \
         WHERE a.id = done.id RETURNING done.envelope",
    )
    .bind(id)
    .bind(hash.as_slice())
    .fetch_optional(&state.pool)
    .await?;
    if let Some(row) = delivered {
        let envelope: Vec<u8> = row.get(0);
        return Ok(Json(
            json!({ "state": "completed", "envelope": b64_text(&envelope) }),
        ));
    }
    let row = sqlx::query(
        "SELECT state, existing_pub FROM approvals \
         WHERE id = $1 AND poll_hash = $2 AND expires_at > now()",
    )
    .bind(id)
    .bind(hash.as_slice())
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    let existing: Option<Vec<u8>> = row.get(1);
    Ok(Json(json!({
        "state": row.get::<String, _>(0),
        "existing_pub": existing.as_deref().map(b64_text),
    })))
}

fn b64_text(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reveal {
    #[serde(with = "b64")]
    new_pub: Vec<u8>,
}

/// `POST /v1/approvals/{id}/reveal` (N): its key, which must match its
/// commitment; only after E accepted.
async fn reveal(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(req): ApiJson<Reveal>,
) -> Result<StatusCode, ApiError> {
    let hash = poll_hash(&headers)?;
    let new_pub = bytes32(&req.new_pub)?;
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query(
        "SELECT state, commitment FROM approvals \
         WHERE id = $1 AND poll_hash = $2 AND expires_at > now() FOR UPDATE",
    )
    .bind(id)
    .bind(hash.as_slice())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;
    if row.get::<String, _>(0) != "accepted" {
        return Err(ApiError::Conflict("wrong_step"));
    }
    let commitment = bytes32(&row.get::<Vec<u8>, _>(1))?;
    check_approval_commitment(&commitment, &new_pub).map_err(|_| ApiError::InvalidRequest)?;
    sqlx::query("UPDATE approvals SET new_pub = $2, state = 'revealed' WHERE id = $1")
        .bind(id)
        .bind(new_pub.as_slice())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /v1/approvals/{id}` (N): gives up.
async fn end(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let hash = poll_hash(&headers)?;
    let ended = sqlx::query(
        "UPDATE approvals SET state = 'ended', envelope = NULL \
         WHERE id = $1 AND poll_hash = $2 AND expires_at > now() RETURNING id",
    )
    .bind(id)
    .bind(hash.as_slice())
    .fetch_optional(&state.pool)
    .await?;
    ended
        .map(|_| StatusCode::NO_CONTENT)
        .ok_or(ApiError::NotFound)
}

#[derive(Serialize)]
struct Listed {
    id: Uuid,
    platform: String,
    state: String,
    created_at: i64,
    #[serde(with = "b64")]
    commitment: Vec<u8>,
    #[serde(with = "b64::option")]
    new_pub: Option<Vec<u8>>,
}

/// `GET /v1/approvals` (E): the account's open requests, oldest first.
async fn list(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<Json<Vec<Listed>>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, platform, state, (extract(epoch FROM created_at) * 1000)::bigint, commitment, new_pub \
         FROM approvals WHERE account_id = $1 \
           AND state IN ('pending', 'accepted', 'revealed') AND expires_at > now() \
         ORDER BY created_at, id",
    )
    .bind(auth.account_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| Listed {
                id: row.get(0),
                platform: row.get(1),
                state: row.get(2),
                created_at: row.get(3),
                commitment: row.get(4),
                new_pub: row.get(5),
            })
            .collect(),
    ))
}

/// Locks one of the account's requests if it is at `from`: 404 when it
/// isn't the account's or is gone, 409 out of order. The caller makes the
/// step and commits.
async fn at_step(
    state: &AppState,
    auth: &Authenticated,
    id: Uuid,
    from: &str,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, ApiError> {
    let mut tx = state.pool.begin().await?;
    let current: Option<String> = sqlx::query_scalar(
        "SELECT state FROM approvals WHERE id = $1 AND account_id = $2 AND expires_at > now() FOR UPDATE",
    )
    .bind(id)
    .bind(auth.account_id)
    .fetch_optional(&mut *tx)
    .await?;
    match current.as_deref() {
        None | Some("delivered" | "ended") => Err(ApiError::NotFound),
        Some(state) if state != from => Err(ApiError::Conflict("wrong_step")),
        Some(_) => Ok(tx),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Accept {
    #[serde(with = "b64")]
    existing_pub: Vec<u8>,
}

/// `POST /v1/approvals/{id}/accept` (E): its fresh key.
async fn accept(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(req): ApiJson<Accept>,
) -> Result<StatusCode, ApiError> {
    let key = bytes32(&req.existing_pub)?;
    let mut tx = at_step(&state, &auth, id, "pending").await?;
    sqlx::query("UPDATE approvals SET state = 'accepted', existing_pub = $2 WHERE id = $1")
        .bind(id)
        .bind(key.as_slice())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    #[serde(with = "b64")]
    envelope: Vec<u8>,
}

/// `POST /v1/approvals/{id}/complete` (E): the sealed Secret Key, after the
/// user saw the codes match.
async fn complete(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(req): ApiJson<Complete>,
) -> Result<StatusCode, ApiError> {
    if req.envelope.len() > 256 || Envelope::from_bytes(&req.envelope).is_err() {
        return Err(ApiError::InvalidRequest);
    }
    let mut tx = at_step(&state, &auth, id, "revealed").await?;
    sqlx::query("UPDATE approvals SET state = 'completed', envelope = $2 WHERE id = $1")
        .bind(id)
        .bind(&req.envelope)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/approvals/{id}/deny` (E): the codes didn't match, or the
/// request isn't the user's.
async fn deny(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    let mut tx = state.pool.begin().await?;
    let ended = sqlx::query(
        "UPDATE approvals SET state = 'ended', envelope = NULL \
         WHERE id = $1 AND account_id = $2 AND expires_at > now() \
           AND state NOT IN ('delivered', 'ended') RETURNING id",
    )
    .bind(id)
    .bind(auth.account_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    ended
        .map(|_| StatusCode::NO_CONTENT)
        .ok_or(ApiError::NotFound)
}
