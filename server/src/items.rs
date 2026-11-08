//! Item writes with revision preconditions, deletes and delta sync (D27).
//!
//! Every write or delete bumps `vaults.seq` while holding that vault's row
//! lock, in the same transaction, so a vault's changes commit in `seq` order
//! and a reader never sees a later change without the earlier ones.

use axum::extract::{DefaultBodyLimit, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use pwm_crypto::Envelope;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::api::{ApiError, ApiJson, ApiPath, ApiRouter, AppState, b64, parse_counter};
use crate::session::Authenticated;

/// The largest item blob, decoded (D29); `pwm_vault::MAX_SEALED_ITEM_BYTES`.
pub const MAX_ITEM_BLOB_BYTES: usize = 256 * 1024;
/// Item write bodies: the blob in base64url plus the JSON around it.
const MAX_ITEM_BODY_BYTES: usize = 384 * 1024;
/// Live items per vault until plans set their own caps (M9).
pub const MAX_ITEMS_PER_VAULT: i64 = 50_000;

const DEFAULT_LIMIT: i64 = 500;
const MAX_LIMIT: i64 = 1000;

pub fn routes() -> ApiRouter {
    Router::new()
        .route("/v1/vaults/{vault_id}/items", get(changes))
        .route(
            "/v1/vaults/{vault_id}/items/{item_id}",
            put(write)
                .delete(remove)
                .layer(DefaultBodyLimit::max(MAX_ITEM_BODY_BYTES)),
        )
}

/// What a write or delete must match.
#[derive(Clone, Copy)]
enum Precondition {
    /// `If-None-Match: *`: the item must not exist yet.
    Create,
    /// `If-Match: "<revision>"`: the item must be at this revision.
    Revision(i64),
}

fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Result<Option<&str>, ApiError> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(ApiError::InvalidRequest);
    }
    value
        .map(|v| v.to_str().map_err(|_| ApiError::InvalidRequest))
        .transpose()
}

/// Reads the precondition. Only the forms D27 defines are accepted: a
/// single strong ETag holding a revision, or `If-None-Match: *`.
fn precondition(headers: &HeaderMap) -> Result<Precondition, ApiError> {
    let if_match = single_header(headers, header::IF_MATCH)?;
    let if_none_match = single_header(headers, header::IF_NONE_MATCH)?;
    match (if_match, if_none_match) {
        (None, None) => Err(ApiError::PreconditionRequired),
        (Some(etag), None) => etag
            .strip_prefix('"')
            .and_then(|e| e.strip_suffix('"'))
            .and_then(parse_counter)
            .filter(|revision| *revision >= 1)
            .map(Precondition::Revision)
            .ok_or(ApiError::InvalidRequest),
        (None, Some("*")) => Ok(Precondition::Create),
        _ => Err(ApiError::InvalidRequest),
    }
}

fn etag(revision: i64) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{revision}\"")).expect("digits are a valid header value")
}

#[derive(Deserialize)]
struct ItemPath {
    vault_id: Uuid,
    item_id: Uuid,
}

#[derive(Deserialize)]
struct ItemWrite {
    revision: String,
    /// The item envelope (D18), sealed with `revision` in its AAD.
    #[serde(with = "b64")]
    blob: Vec<u8>,
}

/// The stored item, returned with a 412 so the client can merge at once.
#[derive(Serialize)]
struct CurrentItem {
    revision: String,
    seq: String,
    #[serde(with = "b64")]
    blob: Vec<u8>,
}

#[derive(Serialize)]
struct Written {
    revision: String,
    seq: String,
}

enum WriteError {
    Api(ApiError),
    /// 412 `revision_conflict` with the current item.
    Conflict {
        revision: i64,
        seq: i64,
        blob: Vec<u8>,
    },
}

impl From<ApiError> for WriteError {
    fn from(err: ApiError) -> Self {
        Self::Api(err)
    }
}

impl From<sqlx::Error> for WriteError {
    fn from(err: sqlx::Error) -> Self {
        Self::Api(err.into())
    }
}

impl IntoResponse for WriteError {
    fn into_response(self) -> Response {
        match self {
            Self::Api(err) => err.into_response(),
            Self::Conflict {
                revision,
                seq,
                blob,
            } => {
                let body = serde_json::json!({
                    "error": "revision_conflict",
                    "current": CurrentItem { revision: revision.to_string(), seq: seq.to_string(), blob },
                });
                let headers = [(header::ETAG, etag(revision))];
                (StatusCode::PRECONDITION_FAILED, headers, Json(body)).into_response()
            }
        }
    }
}

/// Locks the vault for a change by `account_id`; returns its current `seq`.
/// A non-member gets 404, like a vault that does not exist, and takes no
/// lock; a viewer gets 403.
async fn lock_for_write(
    conn: &mut PgConnection,
    vault_id: Uuid,
    account_id: Uuid,
) -> Result<i64, ApiError> {
    let row = sqlx::query(
        "SELECT k.role, v.seq FROM vaults v \
         JOIN vault_keys k ON k.vault_id = v.id AND k.account_id = $2 \
         WHERE v.id = $1 FOR UPDATE OF v",
    )
    .bind(vault_id)
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApiError::NotFound)?;
    if row.get::<&str, _>(0) == "viewer" {
        return Err(ApiError::ReadOnly);
    }
    Ok(row.get(1))
}

/// Ids are never reused: a deleted id answers 409 `item_deleted`.
async fn refuse_deleted(conn: &mut PgConnection, path: &ItemPath) -> Result<(), ApiError> {
    let deleted = sqlx::query("SELECT 1 FROM tombstones WHERE vault_id = $1 AND item_id = $2")
        .bind(path.vault_id)
        .bind(path.item_id)
        .fetch_optional(&mut *conn)
        .await?;
    match deleted {
        Some(_) => Err(ApiError::Conflict("item_deleted")),
        None => Ok(()),
    }
}

/// The item's `(revision, seq, blob)`, if it exists.
async fn current_item(
    conn: &mut PgConnection,
    path: &ItemPath,
) -> Result<Option<(i64, i64, Vec<u8>)>, ApiError> {
    let row = sqlx::query("SELECT revision, seq, blob FROM items WHERE vault_id = $1 AND id = $2")
        .bind(path.vault_id)
        .bind(path.item_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| (r.get(0), r.get(1), r.get(2))))
}

async fn set_vault_seq(conn: &mut PgConnection, vault_id: Uuid, seq: i64) -> Result<(), ApiError> {
    sqlx::query("UPDATE vaults SET seq = $2 WHERE id = $1")
        .bind(vault_id)
        .bind(seq)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// `PUT`: creates (`If-None-Match: *`, revision 1) or replaces
/// (`If-Match: "<current>"`, revision current + 1) one item.
async fn write(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(path): ApiPath<ItemPath>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<ItemWrite>,
) -> Result<Response, WriteError> {
    let precondition = precondition(&headers)?;
    let revision = parse_counter(&body.revision).ok_or(ApiError::InvalidRequest)?;
    let expected = match precondition {
        Precondition::Create => 1,
        Precondition::Revision(current) => {
            current.checked_add(1).ok_or(ApiError::InvalidRequest)?
        }
    };
    if revision != expected {
        return Err(ApiError::InvalidRequest.into());
    }
    if body.blob.len() > MAX_ITEM_BLOB_BYTES {
        return Err(ApiError::TooLarge.into());
    }
    Envelope::from_bytes(&body.blob).map_err(|_| ApiError::InvalidRequest)?;

    let mut tx = state.pool.begin().await?;
    let seq = lock_for_write(&mut tx, path.vault_id, auth.account_id).await? + 1;
    refuse_deleted(&mut tx, &path).await?;
    let status = match (precondition, current_item(&mut tx, &path).await?) {
        (Precondition::Create, None) => {
            // Counted under the vault lock, so racing creates cannot pass it.
            let live: i64 = sqlx::query_scalar("SELECT count(*) FROM items WHERE vault_id = $1")
                .bind(path.vault_id)
                .fetch_one(&mut *tx)
                .await?;
            if live >= MAX_ITEMS_PER_VAULT {
                return Err(ApiError::LimitReached.into());
            }
            sqlx::query(
                "INSERT INTO items (vault_id, id, revision, seq, blob) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(path.vault_id)
            .bind(path.item_id)
            .bind(revision)
            .bind(seq)
            .bind(&body.blob)
            .execute(&mut *tx)
            .await?;
            StatusCode::CREATED
        }
        (Precondition::Revision(_), None) => return Err(ApiError::NotFound.into()),
        (Precondition::Revision(current), Some((stored, _, _))) if stored == current => {
            sqlx::query(
                "UPDATE items SET revision = $3, seq = $4, blob = $5, updated_at = now() \
                 WHERE vault_id = $1 AND id = $2",
            )
            .bind(path.vault_id)
            .bind(path.item_id)
            .bind(revision)
            .bind(seq)
            .bind(&body.blob)
            .execute(&mut *tx)
            .await?;
            StatusCode::OK
        }
        (_, Some((revision, seq, blob))) => {
            return Err(WriteError::Conflict {
                revision,
                seq,
                blob,
            });
        }
    };
    set_vault_seq(&mut tx, path.vault_id, seq).await?;
    tx.commit().await?;
    state.push.vault_changed(path.vault_id, auth.device_id);
    let written = Written {
        revision: revision.to_string(),
        seq: seq.to_string(),
    };
    Ok((status, [(header::ETAG, etag(revision))], Json(written)).into_response())
}

/// `DELETE` with `If-Match: "<current>"`: removes the item and leaves a
/// tombstone with that revision and a new `seq`.
async fn remove(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(path): ApiPath<ItemPath>,
    headers: HeaderMap,
) -> Result<Json<Written>, WriteError> {
    let Precondition::Revision(expected) = precondition(&headers)? else {
        return Err(ApiError::InvalidRequest.into());
    };
    let mut tx = state.pool.begin().await?;
    let seq = lock_for_write(&mut tx, path.vault_id, auth.account_id).await? + 1;
    refuse_deleted(&mut tx, &path).await?;
    match current_item(&mut tx, &path).await? {
        None => return Err(ApiError::NotFound.into()),
        Some((revision, seq, blob)) if revision != expected => {
            return Err(WriteError::Conflict {
                revision,
                seq,
                blob,
            });
        }
        Some(_) => {}
    }
    sqlx::query("DELETE FROM items WHERE vault_id = $1 AND id = $2")
        .bind(path.vault_id)
        .bind(path.item_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO tombstones (vault_id, item_id, revision, seq) VALUES ($1, $2, $3, $4)",
    )
    .bind(path.vault_id)
    .bind(path.item_id)
    .bind(expected)
    .bind(seq)
    .execute(&mut *tx)
    .await?;
    set_vault_seq(&mut tx, path.vault_id, seq).await?;
    tx.commit().await?;
    state.push.vault_changed(path.vault_id, auth.device_id);
    Ok(Json(Written {
        revision: expected.to_string(),
        seq: seq.to_string(),
    }))
}

/// `since` (default 0) and `limit` (default 500, 1 to 1000). Unknown
/// parameters are ignored; a repeated one is refused.
fn sync_params(query: Option<&str>) -> Result<(i64, i64), ApiError> {
    let (mut since, mut limit) = (None, None);
    for pair in query.unwrap_or("").split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').ok_or(ApiError::InvalidRequest)?;
        let slot = match key {
            "since" => &mut since,
            "limit" => &mut limit,
            _ => continue,
        };
        let value = parse_counter(value).ok_or(ApiError::InvalidRequest)?;
        if slot.replace(value).is_some() {
            return Err(ApiError::InvalidRequest);
        }
    }
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(ApiError::InvalidRequest);
    }
    Ok((since.unwrap_or(0), limit))
}

/// An item (with its blob) or a tombstone (`deleted: true`).
#[derive(Serialize)]
struct Change {
    id: Uuid,
    revision: String,
    seq: String,
    #[serde(skip_serializing_if = "Option::is_none", with = "b64::option")]
    blob: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted: Option<bool>,
}

#[derive(Serialize)]
struct Changes {
    changes: Vec<Change>,
    /// The `since` for the next call.
    next: String,
    more: bool,
}

/// `GET ?since=&limit=`: items and tombstones with `seq > since`, in `seq`
/// order, all read from one snapshot. 410 `resync_required` when `since` is
/// older than the vault's compacted tombstones (D76 §2).
async fn changes(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiPath(vault_id): ApiPath<Uuid>,
    RawQuery(query): RawQuery,
) -> Result<Json<Changes>, ApiError> {
    let (since, limit) = sync_params(query.as_deref())?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let (vault_seq, purged_seq): (i64, i64) = sqlx::query_as(
        "SELECT v.seq, v.purged_seq FROM vaults v \
         JOIN vault_keys k ON k.vault_id = v.id AND k.account_id = $2 \
         WHERE v.id = $1",
    )
    .bind(vault_id)
    .bind(auth.account_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;
    // A delete after `since` may have been compacted away (D76 §2).
    if since > 0 && since < purged_seq {
        return Err(ApiError::ResyncRequired);
    }
    let rows = sqlx::query(
        "SELECT id, revision, seq, blob FROM items WHERE vault_id = $1 AND seq > $2 \
         UNION ALL \
         SELECT item_id, revision, seq, NULL FROM tombstones WHERE vault_id = $1 AND seq > $2 \
         ORDER BY seq LIMIT $3",
    )
    .bind(vault_id)
    .bind(since)
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    let more = rows.len() as i64 > limit;
    let changes: Vec<Change> = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| {
            let blob: Option<Vec<u8>> = row.get(3);
            Change {
                id: row.get(0),
                revision: row.get::<i64, _>(1).to_string(),
                seq: row.get::<i64, _>(2).to_string(),
                deleted: blob.is_none().then_some(true),
                blob,
            }
        })
        .collect();
    let next = match changes.last() {
        Some(last) if more => last.seq.clone(),
        _ => vault_seq.to_string(),
    };
    Ok(Json(Changes {
        changes,
        next,
        more,
    }))
}
