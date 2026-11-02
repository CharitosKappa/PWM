//! `POST /v1/vaults` and `GET /v1/vaults` (D27).

use axum::extract::State;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use pwm_crypto::Envelope;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::api::{ApiError, ApiJson, ApiRouter, AppState, b64, is_unique_violation};
use crate::session::Authenticated;

/// Vaults one account may own until plans set their own caps (M9).
pub const MAX_VAULTS_PER_ACCOUNT: i64 = 100;

pub fn routes() -> ApiRouter {
    Router::new().route("/v1/vaults", get(list).post(create))
}

#[derive(Deserialize)]
struct NewVault {
    id: Uuid,
    /// The vault key sealed under the caller's `sym_key` (AAD `pwm/vault-key`).
    #[serde(with = "b64")]
    wrapped_key: Vec<u8>,
}

/// One vault the caller belongs to, with their copy of its key.
#[derive(Serialize)]
struct VaultEntry {
    id: Uuid,
    role: String,
    wrap: String,
    #[serde(with = "b64")]
    wrapped_key: Vec<u8>,
    shared_by: Option<Uuid>,
    /// The vault's last change counter, as a decimal string.
    seq: String,
}

/// Creates a vault owned by the caller. Ids are random UUIDv4 chosen by the
/// client, so a taken id is a client bug, not an existence probe.
async fn create(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiJson(body): ApiJson<NewVault>,
) -> Result<(StatusCode, Json<VaultEntry>), ApiError> {
    Envelope::from_bytes(&body.wrapped_key).map_err(|_| ApiError::InvalidRequest)?;
    let mut tx = state.pool.begin().await?;
    // Counted under the account's row lock, so racing creates cannot pass it.
    sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(auth.account_id)
        .execute(&mut *tx)
        .await?;
    let owned: i64 = sqlx::query_scalar("SELECT count(*) FROM vaults WHERE owner_id = $1")
        .bind(auth.account_id)
        .fetch_one(&mut *tx)
        .await?;
    if owned >= MAX_VAULTS_PER_ACCOUNT {
        return Err(ApiError::LimitReached);
    }
    let inserted = sqlx::query("INSERT INTO vaults (id, owner_id) VALUES ($1, $2)")
        .bind(body.id)
        .bind(auth.account_id)
        .execute(&mut *tx)
        .await;
    match inserted {
        Ok(_) => {}
        Err(err) if is_unique_violation(&err) => return Err(ApiError::Conflict("vault_exists")),
        Err(err) => return Err(err.into()),
    }
    sqlx::query(
        "INSERT INTO vault_keys (vault_id, account_id, role, wrap, wrapped_key) \
         VALUES ($1, $2, 'owner', 'own', $3)",
    )
    .bind(body.id)
    .bind(auth.account_id)
    .bind(&body.wrapped_key)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(VaultEntry {
            id: body.id,
            role: "owner".to_owned(),
            wrap: "own".to_owned(),
            wrapped_key: body.wrapped_key,
            shared_by: None,
            seq: "0".to_owned(),
        }),
    ))
}

/// The caller's vaults, and the server epoch in a `pwm-epoch` header (D30)
/// so clients notice a restore from backup.
async fn list(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<([(HeaderName, HeaderValue); 1], Json<Vec<VaultEntry>>), ApiError> {
    let epoch: Uuid = sqlx::query_scalar("SELECT epoch FROM server_meta")
        .fetch_one(&state.pool)
        .await?;
    let epoch = HeaderValue::from_str(&epoch.to_string()).map_err(|_| ApiError::Internal)?;
    let rows = sqlx::query(
        "SELECT v.id, k.role, k.wrap, k.wrapped_key, k.shared_by, v.seq \
         FROM vault_keys k JOIN vaults v ON v.id = k.vault_id \
         WHERE k.account_id = $1 ORDER BY k.created_at, v.id",
    )
    .bind(auth.account_id)
    .fetch_all(&state.pool)
    .await?;
    let list = rows
        .into_iter()
        .map(|row| VaultEntry {
            id: row.get(0),
            role: row.get(1),
            wrap: row.get(2),
            wrapped_key: row.get(3),
            shared_by: row.get(4),
            seq: row.get::<i64, _>(5).to_string(),
        })
        .collect();
    Ok(([(HeaderName::from_static("pwm-epoch"), epoch)], Json(list)))
}
