//! SQLite cache: ciphertext only, versioned schema (decision D20).
#![cfg(feature = "sqlite")]

use std::path::{Path, PathBuf};

use pwm_crypto::{Envelope, Id, KdfParams, Secret, SymmetricKey, generate_id};
use pwm_store::{
    AccountRecord, ItemRecord, SCHEMA_VERSION, SqliteStore, Store, StoreError, SyncState,
    VaultRecord,
};
use pwm_vault::{Item, ItemId, ItemKind, Login, encrypt_item};

const FIXTURE_V1: &str = include_str!("fixtures/store_v1.sql");

/// A database file under the system temp dir, removed on drop.
struct TempDb(PathBuf);

impl TempDb {
    fn new() -> Self {
        let name: String = generate_id()
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Self(std::env::temp_dir().join(format!("pwm-store-{name}.db")))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let mut p = self.0.clone().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(p);
        }
    }
}

fn envelope(fill: u8) -> Envelope {
    let mut bytes = vec![fill; Envelope::MIN_LEN + 8];
    bytes[0] = Envelope::VERSION;
    Envelope::from_bytes(&bytes).unwrap()
}

fn account() -> AccountRecord {
    AccountRecord {
        account_id: [0xA0; 16],
        salt: [0x50; 16],
        kdf_params: KdfParams::DEFAULT,
        sealed_keyset: envelope(0xAA),
    }
}

const VAULT: Id = [0xB0; 16];

fn vault(id: Id) -> VaultRecord {
    VaultRecord {
        vault_id: id,
        wrapped_key: envelope(0xBB),
        sync_cursor: 0,
    }
}

fn item(n: u8, state: SyncState) -> ItemRecord {
    ItemRecord {
        vault_id: VAULT,
        item_id: ItemId::from_bytes([n; 16]),
        revision: u64::from(n),
        envelope: envelope(n),
        state,
    }
}

fn store_with_vault() -> SqliteStore {
    let mut store = SqliteStore::open_in_memory().unwrap();
    store.put_vault(&vault(VAULT)).unwrap();
    store
}

// --- Records ---

#[test]
fn a_new_store_is_empty() {
    let store = SqliteStore::open_in_memory().unwrap();
    assert_eq!(store.account().unwrap(), None);
    assert!(store.vaults().unwrap().is_empty());
    assert!(store.pending_items().unwrap().is_empty());
}

#[test]
fn account_round_trip() {
    let mut store = SqliteStore::open_in_memory().unwrap();
    store.put_account(&account()).unwrap();
    assert_eq!(store.account().unwrap(), Some(account()));

    let mut raised = account();
    raised.kdf_params.iterations = 4;
    raised.sealed_keyset = envelope(0xAB);
    store.put_account(&raised).unwrap();
    assert_eq!(store.account().unwrap(), Some(raised));
}

#[test]
fn a_different_account_needs_clear_first() {
    let mut store = store_with_vault();
    store.put_account(&account()).unwrap();
    let mut other = account();
    other.account_id = [0xA9; 16];
    assert!(matches!(
        store.put_account(&other),
        Err(StoreError::Invalid(_))
    ));

    store.clear().unwrap();
    assert_eq!(store.account().unwrap(), None);
    assert!(store.vaults().unwrap().is_empty());
    store.put_account(&other).unwrap();
}

#[test]
fn vault_round_trip_and_cursor() {
    let mut store = store_with_vault();
    store.put_items(&[item(1, SyncState::Synced)]).unwrap();
    let mut v = vault(VAULT);
    v.sync_cursor = 99;
    store.put_vault(&v).unwrap();
    store.put_vault(&vault([0xB1; 16])).unwrap();

    let mut vaults = store.vaults().unwrap();
    vaults.sort_by_key(|v| v.vault_id);
    assert_eq!(vaults, [v, vault([0xB1; 16])]);
    assert_eq!(
        store.items(&VAULT).unwrap(),
        [item(1, SyncState::Synced)],
        "updating a vault keeps its items"
    );
}

#[test]
fn deleting_a_vault_deletes_its_items() {
    let mut store = store_with_vault();
    store.put_vault(&vault([0xB1; 16])).unwrap();
    let mut other = item(9, SyncState::Synced);
    other.vault_id = [0xB1; 16];
    store
        .put_items(&[
            item(1, SyncState::Synced),
            item(2, SyncState::Modified),
            other.clone(),
        ])
        .unwrap();

    store.delete_vault(&VAULT).unwrap();
    assert!(store.items(&VAULT).unwrap().is_empty());
    assert!(store.pending_items().unwrap().is_empty());
    assert_eq!(store.items(&[0xB1; 16]).unwrap(), [other]);
}

#[test]
fn item_round_trip_and_replace() {
    let mut store = store_with_vault();
    store
        .put_items(&[item(1, SyncState::Synced), item(2, SyncState::Synced)])
        .unwrap();
    assert_eq!(
        store.item(&VAULT, &ItemId::from_bytes([1; 16])).unwrap(),
        Some(item(1, SyncState::Synced))
    );

    let mut edited = item(1, SyncState::Modified);
    edited.revision = 10;
    edited.envelope = envelope(0xEE);
    store.put_items(std::slice::from_ref(&edited)).unwrap();

    let mut items = store.items(&VAULT).unwrap();
    items.sort_by_key(|i| *i.item_id.as_bytes());
    assert_eq!(items, [edited, item(2, SyncState::Synced)]);
    assert_eq!(
        store.item(&VAULT, &ItemId::from_bytes([7; 16])).unwrap(),
        None
    );
}

#[test]
fn delete_item() {
    let mut store = store_with_vault();
    store.put_items(&[item(1, SyncState::Deleted)]).unwrap();
    store
        .delete_item(&VAULT, &ItemId::from_bytes([1; 16]))
        .unwrap();
    assert!(store.items(&VAULT).unwrap().is_empty());
    // Deleting a missing item is not an error (tombstones can repeat).
    store
        .delete_item(&VAULT, &ItemId::from_bytes([1; 16]))
        .unwrap();
}

#[test]
fn pending_items_are_modified_or_deleted() {
    let mut store = store_with_vault();
    store
        .put_items(&[
            item(1, SyncState::Synced),
            item(2, SyncState::Modified),
            item(3, SyncState::Deleted),
        ])
        .unwrap();
    let mut pending: Vec<u8> = store
        .pending_items()
        .unwrap()
        .iter()
        .map(|i| i.item_id.as_bytes()[0])
        .collect();
    pending.sort();
    assert_eq!(pending, [2, 3]);
}

#[test]
fn a_batch_is_all_or_nothing() {
    let mut store = store_with_vault();
    let mut orphan = item(2, SyncState::Synced);
    orphan.vault_id = [0xEE; 16];
    assert!(matches!(
        store.put_items(&[item(1, SyncState::Synced), orphan]),
        Err(StoreError::Invalid(_))
    ));
    assert!(store.items(&VAULT).unwrap().is_empty());
}

#[test]
fn revisions_must_fit_sqlite_integers() {
    let mut store = store_with_vault();
    let mut big = item(1, SyncState::Synced);
    big.revision = i64::MAX as u64;
    store.put_items(std::slice::from_ref(&big)).unwrap();
    big.revision += 1;
    assert!(matches!(
        store.put_items(&[big]),
        Err(StoreError::Invalid(_))
    ));
    let mut v = vault(VAULT);
    v.sync_cursor = u64::MAX;
    assert!(matches!(store.put_vault(&v), Err(StoreError::Invalid(_))));
}

#[test]
fn data_survives_reopening() {
    let db = TempDb::new();
    {
        let mut store = SqliteStore::open(db.path()).unwrap();
        store.put_account(&account()).unwrap();
        store.put_vault(&vault(VAULT)).unwrap();
        store.put_items(&[item(1, SyncState::Modified)]).unwrap();
    }
    let store = SqliteStore::open(db.path()).unwrap();
    assert_eq!(store.account().unwrap(), Some(account()));
    assert_eq!(store.items(&VAULT).unwrap(), [item(1, SyncState::Modified)]);
}

#[cfg(unix)]
#[test]
fn the_file_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let db = TempDb::new();
    SqliteStore::open(db.path()).unwrap();
    let mode = std::fs::metadata(db.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

// --- Zero knowledge ---

#[test]
fn the_file_holds_no_plaintext() {
    let db = TempDb::new();
    let key = SymmetricKey::from_secret(Secret::new([0x42; 32]));
    let mut login = Login::default();
    login.username = "alice-username-marker".into();
    login.password = "hunter2-password-marker".into();
    let plain = Item::new(ItemKind::Login(login), "Bank-title-marker", 0);
    let item_id = ItemId::from_bytes([1; 16]);
    let sealed = encrypt_item(&key, &VAULT, &item_id, 1, &plain).unwrap();
    {
        let mut store = SqliteStore::open(db.path()).unwrap();
        store.put_vault(&vault(VAULT)).unwrap();
        store
            .put_items(&[ItemRecord {
                vault_id: VAULT,
                item_id,
                revision: 1,
                envelope: sealed.clone(),
                state: SyncState::Modified,
            }])
            .unwrap();
    }
    let bytes = std::fs::read(db.path()).unwrap();
    let ct = sealed.to_bytes();
    assert!(
        bytes.windows(ct.len()).any(|w| w == ct),
        "ciphertext stored"
    );
    for marker in [
        "alice-username-marker",
        "hunter2-password-marker",
        "Bank-title-marker",
    ] {
        assert!(
            !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
            "{marker} leaked"
        );
    }
}

#[test]
fn deleted_ciphertext_is_overwritten() {
    let db = TempDb::new();
    let doomed = item(0x5A, SyncState::Synced);
    let ct = doomed.envelope.to_bytes();
    {
        let mut store = SqliteStore::open(db.path()).unwrap();
        store.put_vault(&vault(VAULT)).unwrap();
        store.put_items(&[doomed]).unwrap();
        store
            .delete_item(&VAULT, &ItemId::from_bytes([0x5A; 16]))
            .unwrap();
    }
    let bytes = std::fs::read(db.path()).unwrap();
    assert!(!bytes.windows(ct.len()).any(|w| w == ct));
}

// --- Schema versions and migrations ---

#[test]
fn a_new_file_gets_the_current_schema() {
    assert_eq!(SCHEMA_VERSION, 1);
    let db = TempDb::new();
    SqliteStore::open(db.path()).unwrap();
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let app_id: u32 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(app_id, 0x5057_4D31, "\"PWM1\"");
}

/// The schema the migrations build must match the frozen v1 fixture exactly,
/// so a shipped migration can never be edited by accident.
#[test]
fn migrations_build_the_frozen_v1_schema() {
    let db = TempDb::new();
    SqliteStore::open(db.path()).unwrap();
    let migrated = schema_of(&rusqlite::Connection::open(db.path()).unwrap());

    let frozen = rusqlite::Connection::open_in_memory().unwrap();
    frozen.execute_batch(FIXTURE_V1).unwrap();
    assert_eq!(migrated, schema_of(&frozen));
}

fn schema_of(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT name, sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| {
        Ok((r.get(0)?, normalize_sql(&r.get::<_, String>(1)?)))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_v1_file_opens_and_reads() {
    let db = TempDb::new();
    rusqlite::Connection::open(db.path())
        .unwrap()
        .execute_batch(FIXTURE_V1)
        .unwrap();

    let store = SqliteStore::open(db.path()).unwrap();
    let acc = store.account().unwrap().unwrap();
    assert_eq!(
        acc.account_id,
        [
            0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD,
            0xAE, 0xAF
        ]
    );
    assert_eq!(acc.kdf_params, KdfParams::DEFAULT);
    assert_eq!(acc.sealed_keyset.to_bytes().len(), Envelope::MIN_LEN);

    let vaults = store.vaults().unwrap();
    assert_eq!(vaults.len(), 1);
    assert_eq!(vaults[0].sync_cursor, 42);
    let vault_id = vaults[0].vault_id;

    let mut items = store.items(&vault_id).unwrap();
    items.sort_by_key(|i| i.revision);
    assert_eq!(
        items
            .iter()
            .map(|i| (i.revision, i.state))
            .collect::<Vec<_>>(),
        [(7, SyncState::Synced), (8, SyncState::Modified)]
    );
    assert_eq!(store.pending_items().unwrap().len(), 1);
}

#[test]
fn a_newer_schema_is_refused_and_left_alone() {
    let db = TempDb::new();
    SqliteStore::open(db.path()).unwrap();
    let future = SCHEMA_VERSION + 1;
    rusqlite::Connection::open(db.path())
        .unwrap()
        .pragma_update(None, "user_version", future)
        .unwrap();

    match SqliteStore::open(db.path()) {
        Err(StoreError::NewerSchema { found, supported }) => {
            assert_eq!((found, supported), (future, SCHEMA_VERSION));
        }
        other => panic!("expected NewerSchema, got {other:?}"),
    }
    let version: u32 = rusqlite::Connection::open(db.path())
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, future);
}

#[test]
fn a_foreign_database_is_refused() {
    let db = TempDb::new();
    {
        let conn = rusqlite::Connection::open(db.path()).unwrap();
        conn.execute_batch("CREATE TABLE notes (body TEXT);")
            .unwrap();
    }
    assert!(matches!(
        SqliteStore::open(db.path()),
        Err(StoreError::NotAStore)
    ));

    let other_app = TempDb::new();
    rusqlite::Connection::open(other_app.path())
        .unwrap()
        .pragma_update(None, "application_id", 0x1234_5678)
        .unwrap();
    assert!(matches!(
        SqliteStore::open(other_app.path()),
        Err(StoreError::NotAStore)
    ));
}

#[test]
fn corrupt_rows_are_reported_not_trusted() {
    let db = TempDb::new();
    {
        let mut store = SqliteStore::open(db.path()).unwrap();
        store.put_vault(&vault(VAULT)).unwrap();
        store.put_items(&[item(1, SyncState::Synced)]).unwrap();
    }
    // An envelope with an unknown version byte.
    rusqlite::Connection::open(db.path())
        .unwrap()
        .execute("UPDATE items SET envelope = X'02'", [])
        .unwrap();
    let store = SqliteStore::open(db.path()).unwrap();
    assert!(matches!(store.items(&VAULT), Err(StoreError::Corrupt)));
}
