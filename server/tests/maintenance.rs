//! Scheduled clean-up and tombstone compaction (D76) against a real
//! Postgres: what goes, what stays, and the 410 a device behind the
//! compacted tombstones gets.

mod common;

use axum::Router;
use axum::http::StatusCode;
use common::flows::*;
use common::{Reply, call, send};
use pwm_crypto::{Aad, SymmetricKey, seal};
use pwm_server::{CleanUp, TOMBSTONE_HORIZON_DAYS, clean_up};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Item record format version bound into the item AAD (D11, D18).
const ITEM_VERSION: u8 = 1;

struct Vault {
    id: Uuid,
    key: SymmetricKey,
}

async fn create_vault(app: &Router, account: &Account) -> Vault {
    let id = new_id();
    let key = SymmetricKey::generate().unwrap();
    let wrapped = account.keyset.wrap_vault_key(&key, id.as_bytes()).unwrap();
    let (status, body) = call(
        app,
        "POST",
        "/v1/vaults",
        Some(account.access()),
        Some(json!({ "id": id, "wrapped_key": b64(&wrapped.to_bytes()) })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Vault { id, key }
}

/// Creates an item at revision 1 and returns its id and `seq`.
async fn create(app: &Router, token: &str, vault: &Vault) -> (Uuid, i64) {
    let item = new_id();
    let aad = Aad::item(ITEM_VERSION, vault.id.as_bytes(), item.as_bytes(), 1);
    let blob = b64(&seal(&vault.key, b"item", &aad).unwrap().to_bytes());
    let reply = send(
        app,
        "PUT",
        &format!("/v1/vaults/{}/items/{item}", vault.id),
        Some(token),
        &[("if-none-match", "*")],
        Some(json!({ "revision": "1", "blob": blob })),
    )
    .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    (item, seq_of(&reply))
}

/// Deletes revision 1 of `item` and returns the tombstone's `seq`.
async fn delete(app: &Router, token: &str, vault: &Uuid, item: &Uuid) -> i64 {
    let reply = send(
        app,
        "DELETE",
        &format!("/v1/vaults/{vault}/items/{item}"),
        Some(token),
        &[("if-match", "\"1\"")],
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    seq_of(&reply)
}

fn seq_of(reply: &Reply) -> i64 {
    reply.body["seq"].as_str().unwrap().parse().unwrap()
}

async fn sync(app: &Router, token: &str, vault: &Uuid, since: i64) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!("/v1/vaults/{vault}/items?since={since}"),
        Some(token),
        None,
    )
    .await
}

/// Moves a tombstone's delete time `days` into the past.
async fn age_tombstone(pool: &PgPool, item: &Uuid, days: i64) {
    sqlx::query(
        "UPDATE tombstones SET deleted_at = now() - make_interval(days => $2::int) \
         WHERE item_id = $1",
    )
    .bind(item)
    .bind(days)
    .execute(pool)
    .await
    .unwrap();
}

async fn purged_seq(pool: &PgPool, vault: &Uuid) -> i64 {
    sqlx::query_scalar("SELECT purged_seq FROM vaults WHERE id = $1")
        .bind(vault)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn clean_up_deletes_expired_rows_only() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let pool = &db.pool;
    let expired = register(&app, "expired@example.com").await;
    let revoked = register(&app, "revoked@example.com").await;
    let active = register(&app, "active@example.com").await;

    // Sessions: one past its refresh token's life, one of a revoked device
    // still within it, one active.
    sqlx::query(
        "UPDATE sessions SET refresh_expires_at = now() - interval '1 second' WHERE device_id = $1",
    )
    .bind(expired.device)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE devices SET revoked_at = now() WHERE id = $1")
        .bind(revoked.device)
        .execute(pool)
        .await
        .unwrap();

    // Login attempts and approvals: one expired, one not, of each.
    for interval in ["-1 second", "60 seconds"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO login_attempts (account_id, state, expires_at) \
             VALUES (gen_random_uuid(), '\\x00', now() + interval '{interval}')"
        )))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO approvals (platform, commitment, poll_hash, expires_at) \
             VALUES ('linux', sha256(gen_random_uuid()::text::bytea), \
                     sha256(gen_random_uuid()::text::bytea), now() + interval '{interval}')"
        )))
        .execute(pool)
        .await
        .unwrap();
    }
    // Autofill reports: one past the 90 days, one recent.
    for age in ["91 days", "1 day"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO autofill_reports (received_at, site, report) \
             VALUES (now() - interval '{age}', 'example.com', '{{}}')"
        )))
        .execute(pool)
        .await
        .unwrap();
    }

    let done = clean_up(pool).await.unwrap();
    assert_eq!(
        done,
        CleanUp {
            sessions: 1,
            login_attempts: 1,
            approvals: 1,
            reports: 1,
            tombstones: 0,
        }
    );
    assert_eq!(count(pool, "sessions").await, 2);
    assert_eq!(count(pool, "login_attempts").await, 1);
    assert_eq!(count(pool, "approvals").await, 1);
    assert_eq!(count(pool, "autofill_reports").await, 1);

    // The revoked device is still told so, and clears its cache (D28).
    assert_eq!(
        refresh(&app, revoked.refresh()).await,
        (StatusCode::FORBIDDEN, json!({ "error": "device_revoked" }))
    );
    assert_eq!(get_account(&app, active.access()).await, StatusCode::OK);
    assert_eq!(refresh(&app, expired.refresh()).await, unauthorized());

    // Running again finds nothing.
    assert_eq!(clean_up(pool).await.unwrap(), CleanUp::default());
    db.drop().await;
}

#[tokio::test]
async fn old_tombstones_are_compacted_and_older_cursors_must_resync() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let pool = &db.pool;
    let account = register(&app, "a@example.com").await;
    let token = account.access();
    let vault = create_vault(&app, &account).await;
    let other = create_vault(&app, &account).await;

    let (old, _) = create(&app, token, &vault).await;
    let (recent, _) = create(&app, token, &vault).await;
    let (kept, kept_seq) = create(&app, token, &vault).await;
    let old_seq = delete(&app, token, &vault.id, &old).await;
    let recent_seq = delete(&app, token, &vault.id, &recent).await;
    let (elsewhere, _) = create(&app, token, &other).await;
    delete(&app, token, &other.id, &elsewhere).await;

    // Just past the horizon, just inside it, and a recent one elsewhere.
    age_tombstone(pool, &old, TOMBSTONE_HORIZON_DAYS + 1).await;
    age_tombstone(pool, &recent, TOMBSTONE_HORIZON_DAYS - 1).await;

    let done = clean_up(pool).await.unwrap();
    assert_eq!(done.tombstones, 1);
    assert_eq!(purged_seq(pool, &vault.id).await, old_seq);
    assert_eq!(purged_seq(pool, &other.id).await, 0);
    assert_eq!(count(pool, "tombstones").await, 2);

    // From 0: the item and the remaining tombstone, not the compacted one.
    let (status, body) = sync(&app, token, &vault.id, 0).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [kept.to_string(), recent.to_string()]);

    // A cursor below the compacted delete may have missed it.
    for since in [1, old_seq - 1] {
        assert_eq!(
            sync(&app, token, &vault.id, since).await,
            (StatusCode::GONE, json!({ "error": "resync_required" })),
            "since={since}"
        );
    }
    // At or after it, nothing was missed.
    for since in [old_seq, kept_seq.max(old_seq), recent_seq] {
        assert_eq!(sync(&app, token, &vault.id, since).await.0, StatusCode::OK);
    }
    // The other vault is untouched.
    assert_eq!(sync(&app, token, &other.id, 1).await.0, StatusCode::OK);

    // Compacting again never lowers purged_seq.
    age_tombstone(pool, &recent, TOMBSTONE_HORIZON_DAYS + 1).await;
    assert_eq!(clean_up(pool).await.unwrap().tombstones, 1);
    assert_eq!(purged_seq(pool, &vault.id).await, recent_seq);
    assert_eq!(clean_up(pool).await.unwrap().tombstones, 0);
    assert_eq!(purged_seq(pool, &vault.id).await, recent_seq);
    db.drop().await;
}

#[tokio::test]
async fn compaction_works_through_batches() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let pool = &db.pool;
    let account = register(&app, "a@example.com").await;
    let vault = create_vault(&app, &account).await;
    // More than one batch of old tombstones, written directly.
    sqlx::query(
        "INSERT INTO tombstones (vault_id, item_id, revision, seq, deleted_at) \
         SELECT $1, gen_random_uuid(), 1, g, now() - interval '200 days' \
         FROM generate_series(1, 10001) g",
    )
    .bind(vault.id)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(clean_up(pool).await.unwrap().tombstones, 10_001);
    assert_eq!(purged_seq(pool, &vault.id).await, 10_001);
    assert_eq!(count(pool, "tombstones").await, 0);
    db.drop().await;
}

#[tokio::test]
async fn compaction_alongside_writes_keeps_recent_tombstones() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let pool = db.pool.clone();
    let account = register(&app, "a@example.com").await;
    let token = account.access().to_owned();
    let vault = create_vault(&app, &account).await;
    let mut old = Vec::new();
    for _ in 0..20 {
        let (item, _) = create(&app, &token, &vault).await;
        old.push((item, delete(&app, &token, &vault.id, &item).await));
    }
    for (item, _) in &old {
        age_tombstone(&pool, item, TOMBSTONE_HORIZON_DAYS + 10).await;
    }
    let highest_old = old.iter().map(|(_, seq)| *seq).max().unwrap();
    let vault_id = vault.id;

    // New deletes while the clean-up runs over and over.
    let writer = {
        let (app, token) = (app.clone(), token.clone());
        tokio::spawn(async move {
            let mut deleted = Vec::new();
            for _ in 0..20 {
                let (item, _) = create(&app, &token, &vault).await;
                deleted.push(delete(&app, &token, &vault.id, &item).await);
            }
            deleted
        })
    };
    let cleaner = {
        let pool = pool.clone();
        tokio::spawn(async move {
            for _ in 0..20 {
                clean_up(&pool).await.unwrap();
            }
        })
    };
    let recent = writer.await.unwrap();
    cleaner.await.unwrap();
    clean_up(&pool).await.unwrap();

    assert_eq!(purged_seq(&pool, &vault_id).await, highest_old);
    let left: Vec<i64> =
        sqlx::query_scalar("SELECT seq FROM tombstones WHERE vault_id = $1 ORDER BY seq")
            .bind(vault_id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(left, recent);
    // The newest cursor still syncs; one from before the old deletes can't.
    assert_eq!(
        sync(&app, &token, &vault_id, highest_old).await.0,
        StatusCode::OK
    );
    assert_eq!(sync(&app, &token, &vault_id, 1).await.0, StatusCode::GONE);
    db.drop().await;
}
