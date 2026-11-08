//! Account deletion (D74) through the HTTP API against a real Postgres: the
//! password proven again, everything deleted, the other devices told, and
//! restores that keep deleted accounts deleted.

mod common;

use axum::Router;
use axum::http::StatusCode;
use common::flows::*;
use common::{TestDb, call, migrated_db};
use pwm_crypto::{ServerSecrets, SymmetricKey};
use pwm_server::{AppState, DeletionLog, Limits, app, purge_deletions, redelete, system_clock};
use serde_json::json;
use uuid::Uuid;

fn account_deleted() -> (StatusCode, serde_json::Value) {
    (StatusCode::FORBIDDEN, json!({ "error": "account_deleted" }))
}

/// Under a session a failed proof is 403, so it isn't taken for an expired
/// access token.
fn not_proven() -> (StatusCode, serde_json::Value) {
    (
        StatusCode::FORBIDDEN,
        json!({ "error": "invalid_credentials" }),
    )
}

async fn count(db: &TestDb, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {sql}")))
        .fetch_one(&db.pool)
        .await
        .unwrap()
}

/// A deletions file of its own, in the system's temp directory.
fn temp_log() -> (DeletionLog, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("pwm-deletions-{}", new_id()));
    (DeletionLog::at(path.clone()), path)
}

async fn setup_with(log: DeletionLog) -> Option<(TestDb, Router)> {
    let db = migrated_db().await?;
    let state = AppState::new(db.pool.clone(), ServerSecrets::generate().unwrap())
        .with_limits(Limits::unlimited(), system_clock())
        .with_deletions(log);
    Some((db, app(state)))
}

async fn create_vault(app: &Router, account: &Account) -> Uuid {
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
    id
}

#[tokio::test]
async fn a_wrong_password_deletes_nothing() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let (_, login_id, _) = delete_start(&app, &alice).await;
    assert_eq!(
        delete_finish(&app, &alice, &login_id, &[7u8; 64]).await,
        not_proven()
    );
    // The attempt was used up.
    let (client, login_id, response) = delete_start(&app, &alice).await;
    let finalization = client
        .finish(&alice.auth_secret, alice.id.as_bytes(), &response)
        .unwrap();
    let (status, _) = delete_finish(&app, &alice, &login_id, &finalization).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        delete_finish(&app, &alice, &login_id, &finalization)
            .await
            .0,
        StatusCode::UNAUTHORIZED,
        "and is gone with the account"
    );
    db.drop().await;
}

#[tokio::test]
async fn no_session_no_deletion() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let (status, _) = call(
        &app,
        "POST",
        "/v1/account/delete/start",
        None,
        Some(json!({ "credential_request": b64(&[0u8; 96]) })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(count(&db, "accounts").await, 1);
    assert_eq!(get_account(&app, alice.access()).await, StatusCode::OK);
    db.drop().await;
}

/// Bob's password proves nothing about Alice's account, even through a
/// login attempt Bob started himself.
#[tokio::test]
async fn another_accounts_attempt_is_refused() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let bob = register(&app, "bob@example.com").await;
    let (client, login_id, response) = login_start(&app, &bob).await;
    let finalization = client
        .finish(&bob.auth_secret, bob.id.as_bytes(), &response)
        .unwrap();
    assert_eq!(
        delete_finish(&app, &alice, &login_id, &finalization).await,
        not_proven()
    );
    assert_eq!(count(&db, "accounts").await, 2);
    db.drop().await;
}

#[tokio::test]
async fn deleting_removes_everything_and_the_other_devices_are_told() {
    let (log, path) = temp_log();
    let Some((db, app)) = setup_with(log.clone()).await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let (status, phone) = login(&app, &alice, &new_id()).await;
    assert_eq!(status, StatusCode::OK);
    create_vault(&app, &alice).await;
    let bob = register(&app, "bob@example.com").await;
    let bob_vault = create_vault(&app, &bob).await;

    let (status, body) = delete_account(&app, &alice).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // Everything Alice had, and nothing of Bob's.
    for table in ["accounts", "devices", "sessions", "vaults", "vault_keys"] {
        assert_eq!(count(&db, table).await, 1, "only Bob's left in {table}");
    }
    assert_eq!(
        count(&db, &format!("vaults WHERE id = '{bob_vault}'")).await,
        1
    );
    assert_eq!(get_account(&app, bob.access()).await, StatusCode::OK);

    // Both of Alice's devices are told when they refresh; their access
    // tokens find nothing.
    assert_eq!(
        get_account(&app, alice.access()).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(refresh(&app, alice.refresh()).await, account_deleted());
    assert_eq!(
        refresh(&app, phone["refresh_token"].as_str().unwrap()).await,
        account_deleted()
    );
    assert_eq!(count(&db, "deleted_sessions").await, 2);
    assert_eq!(
        count(
            &db,
            &format!("deleted_accounts WHERE account_id = '{}'", alice.id)
        )
        .await,
        1
    );
    // An unknown refresh token is still only unauthorized.
    assert_eq!(refresh(&app, bob.refresh()).await.0, StatusCode::OK);
    assert_eq!(
        refresh(&app, "pwmr_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").await,
        unauthorized()
    );

    // The file outside the database lists Alice.
    let listed = log.read().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, alice.id);
    // The email is free again.
    register(&app, "alice@example.com").await;
    db.drop().await;
    let _ = std::fs::remove_file(path);
}

/// What deleting left in the database: no email, device or name.
#[tokio::test]
async fn the_leftovers_name_nobody() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let (status, _) = delete_account(&app, &alice).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text || '.' || column_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name IN ('deleted_sessions', 'deleted_accounts') \
         ORDER BY 1",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        columns,
        [
            "deleted_accounts.account_id",
            "deleted_accounts.deleted_at",
            "deleted_sessions.expires_at",
            "deleted_sessions.refresh_hash",
        ]
    );
    db.drop().await;
}

#[tokio::test]
async fn a_restore_doesnt_bring_a_deleted_account_back() {
    let (log, path) = temp_log();
    let Some((db, app)) = setup_with(log.clone()).await else {
        return;
    };
    // Alice was deleted after the backup: only the file knows. Bob was
    // deleted before it: the restored table lists him. Carol stays.
    let alice = register(&app, "alice@example.com").await;
    let bob = register(&app, "bob@example.com").await;
    let carol = register(&app, "carol@example.com").await;
    log.record(alice.id).await;
    sqlx::query("INSERT INTO deleted_accounts (account_id) VALUES ($1)")
        .bind(bob.id)
        .execute(&db.pool)
        .await
        .unwrap();

    assert_eq!(redelete(&db.pool, &log).await.unwrap(), 2);
    assert_eq!(count(&db, "accounts").await, 1);
    assert_eq!(get_account(&app, carol.access()).await, StatusCode::OK);
    // Both stay listed, and running it again finds nothing more.
    assert_eq!(count(&db, "deleted_accounts").await, 2);
    assert_eq!(redelete(&db.pool, &log).await.unwrap(), 0);
    // Without a file, the table alone still works.
    assert_eq!(redelete(&db.pool, &DeletionLog::off()).await.unwrap(), 0);
    db.drop().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn old_leftovers_are_purged() {
    let (log, path) = temp_log();
    let Some((db, _app)) = setup_with(log.clone()).await else {
        return;
    };
    let (old, recent) = (new_id(), new_id());
    sqlx::query(
        "INSERT INTO deleted_accounts (account_id, deleted_at) \
         VALUES ($1, now() - interval '61 days'), ($2, now() - interval '59 days')",
    )
    .bind(old)
    .bind(recent)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO deleted_sessions (refresh_hash, expires_at) \
         VALUES ($1, now() - interval '1 second'), ($2, now() + interval '1 day')",
    )
    .bind(vec![1u8; 32])
    .bind(vec![2u8; 32])
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(purge_deletions(&db.pool).await.unwrap(), 2);
    assert_eq!(
        count(
            &db,
            &format!("deleted_accounts WHERE account_id = '{recent}'")
        )
        .await,
        1
    );
    assert_eq!(count(&db, "deleted_sessions").await, 1);

    // The file keeps 60 days too, and a line cut short is dropped.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let day = 24 * 3600;
    std::fs::write(
        &path,
        format!(
            "{old} {}\n{recent} {}\n{old}",
            now - 61 * day,
            now - 59 * day
        ),
    )
    .unwrap();
    assert_eq!(log.read().await.unwrap().len(), 1);
    log.prune().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        format!("{recent} {}\n", now - 59 * day)
    );
    db.drop().await;
    let _ = std::fs::remove_file(path);
}
