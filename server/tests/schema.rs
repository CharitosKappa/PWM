//! Schema v1 (D25): migrations, snapshot, the plaintext allow-list and
//! constraints, against a real Postgres (see `common`).

mod common;

use std::fmt::Write as _;

use common::{migrated_db, test_db};
use pwm_server::{MIGRATOR, ServerError, migrate};
use sqlx::migrate::MigrateError;
use sqlx::{AssertSqlSafe, PgPool};

const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/schema.snapshot");

/// Every text-like column the server stores. Anything else is ids, bytes
/// (ciphertext and keys), numbers, booleans or times. A new entry needs a
/// reason: the server never holds item names, URLs, usernames, folder names
/// or tags (CLAUDE.md).
const TEXT_COLUMNS: &[(&str, &str)] = &[
    (
        "accounts.email",
        "login lookup and the notices PRICING.md requires",
    ),
    ("approvals.platform", "enum, CHECK constraint (D46)"),
    ("approvals.state", "enum, CHECK constraint (D46)"),
    (
        "autofill_reports.report",
        "anonymous failure reports (D38): form structure only, schema-checked, no account",
    ),
    (
        "autofill_reports.site",
        "the reported site's origin (D38), not tied to any account",
    ),
    ("devices.platform", "enum, CHECK constraint"),
    ("push_tokens.app", "enum, CHECK constraint (D50)"),
    ("push_tokens.environment", "enum, CHECK constraint (D50)"),
    ("subscriptions.currency", "ISO 4217 code, CHECK constraint"),
    ("subscriptions.plan", "enum, CHECK constraint"),
    ("subscriptions.provider", "enum, CHECK constraint"),
    (
        "subscriptions.provider_ref",
        "the billing provider's subscription id",
    ),
    ("subscriptions.status", "enum, CHECK constraint"),
    ("vault_keys.role", "enum, CHECK constraint"),
    ("vault_keys.wrap", "enum, CHECK constraint"),
];

const A: &str = "00000000-0000-4000-8000-00000000000a";
const B: &str = "00000000-0000-4000-8000-00000000000b";
const C: &str = "00000000-0000-4000-8000-00000000000c";
const VAULT: &str = "00000000-0000-4000-8000-0000000000f1";
const VAULT_B: &str = "00000000-0000-4000-8000-0000000000f2";
const ITEM: &str = "00000000-0000-4000-8000-0000000000e1";
const GONE: &str = "00000000-0000-4000-8000-0000000000e2";

/// `n` bytes as a SQL expression.
fn bytes(n: usize) -> String {
    format!("decode(repeat('ab', {n}), 'hex')")
}

fn account(id: &str, email: &str) -> String {
    format!(
        "INSERT INTO accounts (id, email, kdf_m, kdf_t, kdf_p, salt_pw, sealed_keyset, x25519_pub, ed25519_pub, opaque_record) \
         VALUES ('{id}', '{email}', 65536, 3, 4, {}, {}, {}, {}, {});",
        bytes(16),
        bytes(138),
        bytes(32),
        bytes(32),
        bytes(192)
    )
}

/// Valid rows in every table: account A owns VAULT (shared to B) with one
/// item, one tombstone, a device, a share link and a subscription; B owns
/// VAULT_B, which A shared on to C.
fn seed() -> String {
    let mut sql =
        account(A, "a@example.com") + &account(B, "b@example.com") + &account(C, "c@example.com");
    write!(
        sql,
        "INSERT INTO devices (id, account_id, platform, name_sealed) \
           VALUES ('00000000-0000-4000-8000-0000000000d1', '{A}', 'ios', {seal});
         INSERT INTO vaults (id, owner_id, seq) VALUES ('{VAULT}', '{A}', 2), ('{VAULT_B}', '{B}', 0);
         INSERT INTO vault_keys (vault_id, account_id, role, wrap, wrapped_key, shared_by) VALUES
           ('{VAULT}', '{A}', 'owner', 'own', {seal}, NULL),
           ('{VAULT}', '{B}', 'viewer', 'share', {share}, '{A}'),
           ('{VAULT_B}', '{B}', 'owner', 'own', {seal}, NULL),
           ('{VAULT_B}', '{C}', 'editor', 'share', {share}, '{A}');
         INSERT INTO items (vault_id, id, revision, seq, blob) VALUES ('{VAULT}', '{ITEM}', 1, 1, {blob});
         INSERT INTO tombstones (vault_id, item_id, revision, seq) VALUES ('{VAULT}', '{GONE}', 3, 2);
         INSERT INTO shares (id, account_id, blob, expires_at, max_views) \
           VALUES ('00000000-0000-4000-8000-0000000000c1', '{A}', {seal}, now() + interval '1 day', 1);
         INSERT INTO subscriptions (account_id, plan, status, provider, provider_ref, price_minor, currency, current_period_end) \
           VALUES ('{A}', 'premium', 'active', 'stripe', 'sub_123', 2400, 'USD', now() + interval '1 year');",
        seal = bytes(73),
        share = bytes(203),
        blob = bytes(297),
    )
    .unwrap();
    sql
}

async fn exec(pool: &PgPool, sql: String) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(AssertSqlSafe(sql))
        .execute(pool)
        .await
        .map(|_| ())
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn migrations_apply_and_rerunning_them_is_a_no_op() {
    let Some(db) = test_db().await else { return };
    migrate(&db.pool).await.unwrap();
    migrate(&db.pool).await.unwrap();
    let applied = count(&db.pool, "_sqlx_migrations WHERE success").await;
    assert_eq!(applied, i64::try_from(MIGRATOR.iter().count()).unwrap());
    db.drop().await;
}

#[tokio::test]
async fn a_migration_from_a_newer_server_is_refused() {
    let Some(db) = migrated_db().await else {
        return;
    };
    exec(
        &db.pool,
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (99990101000000, 'from a newer server', true, '\\x00', 0)"
            .into(),
    )
    .await
    .unwrap();
    let err = migrate(&db.pool).await.unwrap_err();
    assert!(
        matches!(
            err,
            ServerError::Migrate(MigrateError::VersionMissing(99_990_101_000_000))
        ),
        "{err}"
    );
    db.drop().await;
}

#[tokio::test]
async fn an_edited_migration_is_refused() {
    let Some(db) = migrated_db().await else {
        return;
    };
    exec(
        &db.pool,
        "UPDATE _sqlx_migrations SET checksum = '\\x00' WHERE version = 1".into(),
    )
    .await
    .unwrap();
    let err = migrate(&db.pool).await.unwrap_err();
    assert!(
        matches!(err, ServerError::Migrate(MigrateError::VersionMismatch(1))),
        "{err}"
    );
    db.drop().await;
}

async fn describe_schema(pool: &PgPool) -> String {
    let columns: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text, data_type::text, is_nullable::text, \
                coalesce(column_default::text, '') \
         FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations' \
         ORDER BY table_name, ordinal_position",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let constraints: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT c.conrelid::regclass::text, c.conname::text, pg_get_constraintdef(c.oid) \
         FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace \
         WHERE n.nspname = 'public' AND c.conrelid <> 0 \
           AND c.conrelid::regclass::text <> '_sqlx_migrations' \
         ORDER BY 1, 2",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let indexes: Vec<(String, String)> = sqlx::query_as(
        "SELECT tablename::text, indexdef FROM pg_indexes \
         WHERE schemaname = 'public' AND tablename <> '_sqlx_migrations' \
         ORDER BY tablename, indexname",
    )
    .fetch_all(pool)
    .await
    .unwrap();

    let mut out = String::from(
        "# Schema snapshot (D25). Regenerate with PWM_UPDATE_SNAPSHOT=1 after adding a migration.\n",
    );
    let mut tables: Vec<&str> = columns.iter().map(|c| c.0.as_str()).collect();
    tables.dedup();
    for table in tables {
        writeln!(out, "\ntable {table}").unwrap();
        for (_, name, ty, nullable, default) in columns.iter().filter(|c| c.0 == table) {
            let null = if nullable == "YES" {
                "null"
            } else {
                "not null"
            };
            let default = if default.is_empty() {
                String::new()
            } else {
                format!(" default {default}")
            };
            writeln!(out, "  column {name} {ty} {null}{default}").unwrap();
        }
        for (_, name, def) in constraints.iter().filter(|c| c.0 == table) {
            writeln!(out, "  constraint {name}: {def}").unwrap();
        }
        for (_, def) in indexes.iter().filter(|i| i.0 == table) {
            writeln!(out, "  index {def}").unwrap();
        }
    }
    out
}

#[tokio::test]
async fn schema_matches_the_snapshot() {
    let Some(db) = migrated_db().await else {
        return;
    };
    let actual = describe_schema(&db.pool).await;
    if std::env::var("PWM_UPDATE_SNAPSHOT").as_deref() == Ok("1") {
        std::fs::write(SNAPSHOT, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(SNAPSHOT)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "schema differs from tests/schema.snapshot; schema changes need a new migration. \
         Actual schema:\n{actual}"
    );
    db.drop().await;
}

#[tokio::test]
async fn only_allow_listed_columns_hold_text() {
    let Some(db) = migrated_db().await else {
        return;
    };
    let text: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text || '.' || column_name::text \
         FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations' \
           AND data_type IN ('text', 'character varying', 'character', 'json', 'jsonb', \
                             'xml', 'ARRAY', 'USER-DEFINED', 'tsvector') \
         ORDER BY 1",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    let allowed: Vec<&str> = TEXT_COLUMNS.iter().map(|(c, _)| *c).collect();
    assert_eq!(text, allowed);
    db.drop().await;
}

#[tokio::test]
async fn constraints_reject_invalid_rows() {
    let Some(db) = migrated_db().await else {
        return;
    };
    exec(&db.pool, seed()).await.unwrap();

    // (SQL, SQLSTATE): 23514 check, 23505 unique, 23503 foreign key.
    let cases: Vec<(String, &str)> = vec![
        (
            format!(
                "UPDATE accounts SET salt_pw = {} WHERE id = '{A}'",
                bytes(15)
            ),
            "23514",
        ),
        (
            format!(
                "UPDATE accounts SET x25519_pub = {} WHERE id = '{A}'",
                bytes(31)
            ),
            "23514",
        ),
        (
            format!(
                "UPDATE accounts SET ed25519_pub = {} WHERE id = '{A}'",
                bytes(33)
            ),
            "23514",
        ),
        (
            format!(
                "UPDATE accounts SET sealed_keyset = {} WHERE id = '{A}'",
                bytes(40)
            ),
            "23514",
        ),
        (
            format!("UPDATE accounts SET kdf_m = 0 WHERE id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE accounts SET email = ' a@example.com' WHERE id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE accounts SET email = 'nobody' WHERE id = '{A}'"),
            "23514",
        ),
        (
            account("00000000-0000-4000-8000-0000000000aa", "a@example.com"),
            "23505",
        ),
        (
            format!("UPDATE devices SET platform = 'symbian' WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!(
                "UPDATE devices SET name_sealed = {} WHERE account_id = '{A}'",
                bytes(40)
            ),
            "23514",
        ),
        (
            "INSERT INTO devices (id, account_id, platform) \
             VALUES (gen_random_uuid(), gen_random_uuid(), 'ios')"
                .into(),
            "23503",
        ),
        (
            format!("UPDATE vaults SET seq = -1 WHERE id = '{VAULT}'"),
            "23514",
        ),
        (
            format!("UPDATE vault_keys SET role = 'admin' WHERE account_id = '{B}'"),
            "23514",
        ),
        (
            format!("UPDATE vault_keys SET wrap = 'plain' WHERE account_id = '{B}'"),
            "23514",
        ),
        (
            format!(
                "UPDATE vault_keys SET shared_by = '{B}' WHERE account_id = '{A}' AND vault_id = '{VAULT}'"
            ),
            "23514",
        ),
        (
            format!(
                "UPDATE vault_keys SET wrapped_key = {} WHERE account_id = '{B}'",
                bytes(40)
            ),
            "23514",
        ),
        (
            format!("UPDATE items SET revision = 0 WHERE id = '{ITEM}'"),
            "23514",
        ),
        (
            format!("UPDATE items SET seq = 0 WHERE id = '{ITEM}'"),
            "23514",
        ),
        (
            format!("UPDATE items SET blob = {} WHERE id = '{ITEM}'", bytes(40)),
            "23514",
        ),
        (
            format!(
                "INSERT INTO items (vault_id, id, revision, seq, blob) \
                 VALUES ('{VAULT}', gen_random_uuid(), 1, 1, {})",
                bytes(41)
            ),
            "23505",
        ),
        (
            format!(
                "INSERT INTO items (vault_id, id, revision, seq, blob) \
                 VALUES (gen_random_uuid(), gen_random_uuid(), 1, 1, {})",
                bytes(41)
            ),
            "23503",
        ),
        (
            format!("UPDATE tombstones SET revision = 0 WHERE item_id = '{GONE}'"),
            "23514",
        ),
        (
            format!(
                "INSERT INTO tombstones (vault_id, item_id, revision, seq) VALUES ('{VAULT}', gen_random_uuid(), 1, 2)"
            ),
            "23505",
        ),
        (
            format!("UPDATE shares SET views = 2 WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE shares SET max_views = 0 WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!(
                "UPDATE shares SET blob = {} WHERE account_id = '{A}'",
                bytes(40)
            ),
            "23514",
        ),
        (
            format!("UPDATE subscriptions SET plan = 'free' WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE subscriptions SET status = 'trial' WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE subscriptions SET provider = 'paypal' WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE subscriptions SET provider_ref = '' WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE subscriptions SET currency = 'usd' WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!("UPDATE subscriptions SET price_minor = -1 WHERE account_id = '{A}'"),
            "23514",
        ),
        (
            format!(
                "INSERT INTO subscriptions (account_id, plan, status, provider, provider_ref, price_minor, currency, current_period_end) \
                 VALUES ('{B}', 'premium', 'active', 'stripe', 'sub_123', 2400, 'USD', now())"
            ),
            "23505",
        ),
    ];
    for (sql, code) in cases {
        let err = exec(&db.pool, sql.clone())
            .await
            .expect_err(&format!("accepted: {sql}"));
        let actual = err
            .as_database_error()
            .and_then(|e| e.code())
            .map(|c| c.into_owned());
        assert_eq!(actual.as_deref(), Some(code), "{sql}: {err}");
    }
    db.drop().await;
}

/// Rows written under schema v1 survive the v2 migration (D26).
#[tokio::test]
async fn v1_data_survives_the_v2_migration() {
    let Some(db) = test_db().await else { return };
    MIGRATOR.run_to(1, &db.pool).await.unwrap();
    exec(&db.pool, seed()).await.unwrap();
    let before = describe_rows(&db.pool).await;
    migrate(&db.pool).await.unwrap();
    assert_eq!(describe_rows(&db.pool).await, before);
    assert_eq!(count(&db.pool, "sessions").await, 0);
    db.drop().await;
}

/// Row counts per v1 table plus the accounts' records, to compare across a
/// migration.
async fn describe_rows(pool: &PgPool) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    for table in [
        "accounts",
        "devices",
        "vaults",
        "vault_keys",
        "items",
        "tombstones",
        "shares",
        "subscriptions",
    ] {
        out.push((table.to_owned(), count(pool, table).await));
    }
    let records: i64 =
        sqlx::query_scalar("SELECT count(*) FROM accounts WHERE octet_length(opaque_record) = 192")
            .fetch_one(pool)
            .await
            .unwrap();
    out.push(("opaque_record".to_owned(), records));
    out
}

#[tokio::test]
async fn deleting_an_account_removes_everything_it_owns() {
    let Some(db) = migrated_db().await else {
        return;
    };
    exec(&db.pool, seed()).await.unwrap();
    exec(&db.pool, format!("DELETE FROM accounts WHERE id = '{A}'"))
        .await
        .unwrap();

    for table in ["devices", "items", "tombstones", "shares", "subscriptions"] {
        assert_eq!(count(&db.pool, table).await, 0, "{table}");
    }
    // A's vault (with B's share of it) is gone; B's own vault stays, and the
    // share A made of it keeps its row without a sharer.
    assert_eq!(count(&db.pool, "accounts").await, 2);
    assert_eq!(
        count(&db.pool, &format!("vaults WHERE id = '{VAULT_B}'")).await,
        1
    );
    assert_eq!(count(&db.pool, "vaults").await, 1);
    assert_eq!(count(&db.pool, "vault_keys").await, 2);
    assert_eq!(
        count(
            &db.pool,
            &format!("vault_keys WHERE account_id = '{C}' AND shared_by IS NULL")
        )
        .await,
        1
    );
    db.drop().await;
}
