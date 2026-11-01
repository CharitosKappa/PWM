//! SQLite backend (bundled SQLite) with ordered schema migrations.

use std::path::Path;
use std::time::Duration;

use pwm_crypto::{Envelope, Id, KdfParams};
use pwm_vault::ItemId;
use rusqlite::{Connection, ErrorCode, OptionalExtension, Params, TransactionBehavior, params};

use crate::{AccountRecord, ItemRecord, Store, StoreError, SyncState, VaultRecord};

/// Migration `n` (0-based) takes the schema from version `n` to `n + 1`.
/// Shipped entries are never edited.
const MIGRATIONS: &[&str] = &[include_str!("migrations/v1.sql")];

/// Schema version this build writes and the newest it can open.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// `PRAGMA application_id` marking a file as ours: "PWM1".
const APPLICATION_ID: i64 = 0x5057_4D31;

const ITEM_COLUMNS: &str = "vault_id, item_id, revision, envelope, state";

#[derive(Debug)]
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens or creates the cache at `path` and migrates it to
    /// [`SCHEMA_VERSION`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        create_private(path)?;
        Self::init(Connection::open(path)?)
    }

    /// A cache that lives only in memory (tests, locked-down contexts).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StoreError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "secure_delete", true)?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    fn query_items(&self, sql: &str, params: impl Params) -> Result<Vec<ItemRecord>, StoreError> {
        let mut stmt = self.conn.prepare_cached(sql)?;
        let rows = stmt.query_map(params, |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (vault_id, item_id, revision, envelope, state) = row?;
            Ok(ItemRecord {
                vault_id: id(vault_id)?,
                item_id: ItemId::from_bytes(id(item_id)?),
                revision: u64::try_from(revision).map_err(|_| StoreError::Corrupt)?,
                envelope: envelope_from(&envelope)?,
                state: state_from_code(state).ok_or(StoreError::Corrupt)?,
            })
        })
        .collect()
    }
}

/// Applies pending migrations in one transaction, so a crash leaves the file
/// at its old version. Refuses foreign and newer databases without writing.
fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let app_id: i64 = tx.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if app_id == 0 {
        let objects: i64 = tx.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))?;
        if version != 0 || objects != 0 {
            return Err(StoreError::NotAStore);
        }
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    } else if app_id != APPLICATION_ID {
        return Err(StoreError::NotAStore);
    }
    let version = u32::try_from(version).map_err(|_| StoreError::Corrupt)?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    for (step, sql) in (1..).zip(MIGRATIONS).skip(version as usize) {
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", step)?;
    }
    tx.commit()?;
    Ok(())
}

impl Store for SqliteStore {
    fn account(&self) -> Result<Option<AccountRecord>, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT account_id, salt, kdf_memory_kib, kdf_iterations, kdf_parallelism, \
                 sealed_keyset FROM account",
                [],
                |r| {
                    Ok((
                        r.get::<_, Vec<u8>>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, Vec<u8>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((account_id, salt, memory, iterations, parallelism, keyset)) = row else {
            return Ok(None);
        };
        Ok(Some(AccountRecord {
            account_id: id(account_id)?,
            salt: id(salt)?,
            kdf_params: KdfParams {
                memory_kib: stored_u32(memory)?,
                iterations: stored_u32(iterations)?,
                parallelism: stored_u32(parallelism)?,
            },
            sealed_keyset: envelope_from(&keyset)?,
        }))
    }

    fn put_account(&mut self, account: &AccountRecord) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        let stored: Option<Vec<u8>> = tx
            .query_row("SELECT account_id FROM account", [], |r| r.get(0))
            .optional()?;
        if stored.is_some_and(|id| id != account.account_id) {
            return Err(StoreError::Invalid(
                "a different account is stored; clear the store first",
            ));
        }
        let p = &account.kdf_params;
        tx.execute(
            "INSERT OR REPLACE INTO account VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account.account_id,
                account.salt,
                p.memory_kib,
                p.iterations,
                p.parallelism,
                account.sealed_keyset.to_bytes(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn vaults(&self) -> Result<Vec<VaultRecord>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT vault_id, wrapped_key, sync_cursor FROM vaults")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (vault_id, wrapped_key, cursor) = row?;
            Ok(VaultRecord {
                vault_id: id(vault_id)?,
                wrapped_key: envelope_from(&wrapped_key)?,
                sync_cursor: u64::try_from(cursor).map_err(|_| StoreError::Corrupt)?,
            })
        })
        .collect()
    }

    fn put_vault(&mut self, vault: &VaultRecord) -> Result<(), StoreError> {
        let cursor = to_sql(vault.sync_cursor, "sync cursor above i64::MAX")?;
        // An upsert, not INSERT OR REPLACE: replacing would delete the row and
        // cascade to the vault's items.
        self.conn.execute(
            "INSERT INTO vaults VALUES (?1, ?2, ?3) ON CONFLICT (vault_id) DO UPDATE \
             SET wrapped_key = excluded.wrapped_key, sync_cursor = excluded.sync_cursor",
            params![vault.vault_id, vault.wrapped_key.to_bytes(), cursor],
        )?;
        Ok(())
    }

    fn delete_vault(&mut self, vault_id: &Id) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM vaults WHERE vault_id = ?1", [vault_id])?;
        Ok(())
    }

    fn items(&self, vault_id: &Id) -> Result<Vec<ItemRecord>, StoreError> {
        self.query_items(
            &format!("SELECT {ITEM_COLUMNS} FROM items WHERE vault_id = ?1"),
            [vault_id],
        )
    }

    fn item(&self, vault_id: &Id, item_id: &ItemId) -> Result<Option<ItemRecord>, StoreError> {
        Ok(self
            .query_items(
                &format!("SELECT {ITEM_COLUMNS} FROM items WHERE vault_id = ?1 AND item_id = ?2"),
                [vault_id, item_id.as_bytes()],
            )?
            .pop())
    }

    fn put_items(&mut self, items: &[ItemRecord]) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(&format!(
                "INSERT INTO items ({ITEM_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT (vault_id, item_id) DO UPDATE SET revision = excluded.revision, \
                 envelope = excluded.envelope, state = excluded.state"
            ))?;
            for item in items {
                let revision = to_sql(item.revision, "revision above i64::MAX")?;
                stmt.execute(params![
                    item.vault_id,
                    item.item_id.as_bytes(),
                    revision,
                    item.envelope.to_bytes(),
                    state_code(item.state),
                ])
                .map_err(|e| match e.sqlite_error_code() {
                    Some(ErrorCode::ConstraintViolation) => {
                        StoreError::Invalid("item's vault is not stored")
                    }
                    _ => e.into(),
                })?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn delete_item(&mut self, vault_id: &Id, item_id: &ItemId) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM items WHERE vault_id = ?1 AND item_id = ?2",
            [vault_id, item_id.as_bytes()],
        )?;
        Ok(())
    }

    fn pending_items(&self) -> Result<Vec<ItemRecord>, StoreError> {
        self.query_items(
            &format!("SELECT {ITEM_COLUMNS} FROM items WHERE state <> 0"),
            [],
        )
    }

    fn clear(&mut self) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch("DELETE FROM items; DELETE FROM vaults; DELETE FROM account;")?;
        tx.commit()?;
        Ok(())
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        // rusqlite messages name tables and constraints, never bound values.
        Self::Backend(e.to_string())
    }
}

fn id<const N: usize>(bytes: Vec<u8>) -> Result<[u8; N], StoreError> {
    bytes.try_into().map_err(|_| StoreError::Corrupt)
}

fn envelope_from(bytes: &[u8]) -> Result<Envelope, StoreError> {
    Envelope::from_bytes(bytes).map_err(|_| StoreError::Corrupt)
}

/// Stored as `items.state`; the codes are part of the schema.
fn state_code(state: SyncState) -> i64 {
    match state {
        SyncState::Synced => 0,
        SyncState::Modified => 1,
        SyncState::Deleted => 2,
    }
}

fn state_from_code(code: i64) -> Option<SyncState> {
    match code {
        0 => Some(SyncState::Synced),
        1 => Some(SyncState::Modified),
        2 => Some(SyncState::Deleted),
        _ => None,
    }
}

fn stored_u32(value: i64) -> Result<u32, StoreError> {
    u32::try_from(value).map_err(|_| StoreError::Corrupt)
}

fn to_sql(value: u64, what: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Invalid(what))
}

/// Creates the file readable only by its owner before SQLite opens it;
/// SQLite gives its journal files the same mode.
#[cfg(unix)]
fn create_private(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(StoreError::Backend(e.to_string())),
    }
}

/// Windows and mobile apps keep the cache inside the app's private data
/// directory; no extra mode bits apply.
#[cfg(not(unix))]
fn create_private(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}
