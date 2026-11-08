//! The in-memory cache and its snapshot format (decisions D20, D31): the
//! same rules as the SQLite cache, and a versioned snapshot the extension
//! keeps in IndexedDB.

use pwm_crypto::{Envelope, Id, KdfParams};
use pwm_store::{
    AccountRecord, ItemRecord, MemoryStore, PlanRecord, SNAPSHOT_VERSION, Store, StoreError,
    SyncState, VaultRecord,
};
use pwm_vault::ItemId;

const VAULT: Id = [0xB0; 16];

fn envelope(fill: u8) -> Envelope {
    let mut bytes = vec![fill; Envelope::MIN_LEN];
    bytes[0] = 1;
    Envelope::from_bytes(&bytes).unwrap()
}

fn account(id: u8) -> AccountRecord {
    AccountRecord {
        account_id: [id; 16],
        salt: [0x5A; 16],
        kdf_params: KdfParams::DEFAULT,
        sealed_keyset: envelope(0x11),
    }
}

fn vault(id: Id, cursor: u64) -> VaultRecord {
    VaultRecord {
        vault_id: id,
        wrapped_key: envelope(0x22),
        sync_cursor: cursor,
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

fn plan() -> PlanRecord {
    PlanRecord {
        plan: "premium".to_owned(),
        until_ms: Some(1_800_000_000_000),
    }
}

/// A store holding one of everything.
fn filled() -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_account(&account(0xA0)).unwrap();
    store.set_server_epoch(&[0xE0; 16]).unwrap();
    store.set_plan(&plan()).unwrap();
    store.put_vault(&vault(VAULT, 42)).unwrap();
    store.put_vault(&vault([0xB1; 16], 0)).unwrap();
    store
        .put_items(&[
            item(1, SyncState::Synced),
            item(2, SyncState::Modified),
            item(3, SyncState::Deleted),
        ])
        .unwrap();
    store
}

#[test]
fn behaves_like_the_sqlite_cache() {
    let mut store = MemoryStore::new();
    assert_eq!(store.account().unwrap(), None);
    assert_eq!(store.server_epoch().unwrap(), None);

    store.put_account(&account(0xA0)).unwrap();
    assert!(matches!(
        store.put_account(&account(0xA1)),
        Err(StoreError::Invalid(_))
    ));
    // Items need their vault.
    assert!(matches!(
        store.put_items(&[item(1, SyncState::Synced)]),
        Err(StoreError::Invalid(_))
    ));
    store.put_vault(&vault(VAULT, 1)).unwrap();
    // A batch is all or nothing.
    let mut stray = item(9, SyncState::Synced);
    stray.vault_id = [0xFF; 16];
    assert!(
        store
            .put_items(&[item(1, SyncState::Synced), stray])
            .is_err()
    );
    assert!(store.items(&VAULT).unwrap().is_empty());

    store
        .put_items(&[item(1, SyncState::Synced), item(2, SyncState::Modified)])
        .unwrap();
    let pending = store.pending_items().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].item_id, ItemId::from_bytes([2; 16]));
    // Updating a vault keeps its items; deleting it removes them.
    store.put_vault(&vault(VAULT, 7)).unwrap();
    assert_eq!(store.items(&VAULT).unwrap().len(), 2);
    store
        .delete_item(&VAULT, &ItemId::from_bytes([1; 16]))
        .unwrap();
    store
        .delete_item(&VAULT, &ItemId::from_bytes([1; 16]))
        .unwrap();
    assert_eq!(
        store.item(&VAULT, &ItemId::from_bytes([1; 16])).unwrap(),
        None
    );
    store.delete_vault(&VAULT).unwrap();
    assert!(store.pending_items().unwrap().is_empty());

    // Values SQLite cannot hold are refused here too.
    let mut huge = item(1, SyncState::Synced);
    huge.revision = u64::MAX;
    store.put_vault(&vault(VAULT, 0)).unwrap();
    assert!(matches!(
        store.put_items(&[huge]),
        Err(StoreError::Invalid(_))
    ));
    assert!(matches!(
        store.put_vault(&vault(VAULT, u64::MAX)),
        Err(StoreError::Invalid(_))
    ));

    store.set_server_epoch(&[1; 16]).unwrap();
    store.clear().unwrap();
    assert_eq!(store.account().unwrap(), None);
    assert!(store.vaults().unwrap().is_empty());
    assert_eq!(store.server_epoch().unwrap(), None);
    // After clearing, another account may be stored.
    store.put_account(&account(0xA1)).unwrap();
}

#[test]
fn a_snapshot_loads_back_identical() {
    let store = filled();
    let snapshot = store.snapshot();
    assert_eq!(&snapshot[..4], b"PWMS");
    assert_eq!(snapshot[4], SNAPSHOT_VERSION);
    let loaded = MemoryStore::load(&snapshot).unwrap();
    assert_eq!(loaded.account().unwrap(), store.account().unwrap());
    assert_eq!(loaded.server_epoch().unwrap(), Some([0xE0; 16]));
    let mut vaults = loaded.vaults().unwrap();
    vaults.sort_by_key(|v| v.vault_id);
    assert_eq!(vaults, [vault(VAULT, 42), vault([0xB1; 16], 0)]);
    let mut items = loaded.items(&VAULT).unwrap();
    items.sort_by_key(|i| i.revision);
    assert_eq!(
        items,
        [
            item(1, SyncState::Synced),
            item(2, SyncState::Modified),
            item(3, SyncState::Deleted)
        ]
    );
    // Deterministic: the same contents give the same bytes.
    assert_eq!(loaded.snapshot(), snapshot);
    // An empty store too.
    let empty = MemoryStore::new().snapshot();
    assert_eq!(MemoryStore::load(&empty).unwrap().snapshot(), empty);
}

/// A frozen v1 snapshot (hex). Never edit: later versions must load it.
const SNAPSHOT_V1: &str = include_str!("fixtures/snapshot_v1.hex");

#[test]
fn the_frozen_v1_snapshot_loads() {
    let bytes = hex(SNAPSHOT_V1);
    let mut store = MemoryStore::load(&bytes).unwrap();
    assert_eq!(store.account().unwrap(), Some(account(0xA0)));
    assert_eq!(store.server_epoch().unwrap(), Some([0xE0; 16]));
    assert_eq!(store.vaults().unwrap().len(), 2);
    assert_eq!(store.items(&VAULT).unwrap().len(), 3);
    // v1 had no plan: unknown until the next sync (D73 §2).
    assert_eq!(store.plan().unwrap(), None);
    // With the plan the next sync brings, it saves as v2.
    store.set_plan(&plan()).unwrap();
    assert_eq!(store.snapshot(), hex(SNAPSHOT_V2));
}

/// A frozen v2 snapshot (hex), with a plan (D73). Never edit.
const SNAPSHOT_V2: &str = include_str!("fixtures/snapshot_v2.hex");

#[test]
fn the_frozen_v2_snapshot_loads() {
    let bytes = hex(SNAPSHOT_V2);
    let store = MemoryStore::load(&bytes).unwrap();
    assert_eq!(store.plan().unwrap(), Some(plan()));
    assert_eq!(store.items(&VAULT).unwrap().len(), 3);
    assert_eq!(store.snapshot(), bytes);
}

#[test]
fn a_plan_without_an_end_and_a_bad_name() {
    let mut store = MemoryStore::new();
    let free = PlanRecord {
        plan: "free".to_owned(),
        until_ms: None,
    };
    store.set_plan(&free).unwrap();
    let loaded = MemoryStore::load(&store.snapshot()).unwrap();
    assert_eq!(loaded.plan().unwrap(), Some(free));
    let long = PlanRecord {
        plan: "x".repeat(33),
        until_ms: None,
    };
    assert!(store.set_plan(&long).is_err());
    store.clear().unwrap();
    assert_eq!(store.plan().unwrap(), None);
}

/// Prints the fixture; run once with `--ignored --nocapture` when the format
/// version changes (never to edit an existing fixture).
#[test]
#[ignore = "writes a new fixture"]
fn print_snapshot_fixture() {
    let text: String = filled()
        .snapshot()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!("{text}");
}

fn hex(text: &str) -> Vec<u8> {
    let text: String = text.split_whitespace().collect();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn damaged_snapshots_are_refused() {
    let good = filled().snapshot();
    assert!(matches!(
        MemoryStore::load(b"nope"),
        Err(StoreError::NotAStore)
    ));
    assert!(matches!(MemoryStore::load(b""), Err(StoreError::NotAStore)));
    let mut newer = good.clone();
    newer[4] = SNAPSHOT_VERSION + 1;
    assert!(matches!(
        MemoryStore::load(&newer),
        Err(StoreError::NewerSchema { .. })
    ));
    // Every truncation, and trailing bytes, are corrupt.
    for len in 5..good.len() {
        assert!(
            MemoryStore::load(&good[..len]).is_err(),
            "truncated to {len}"
        );
    }
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(matches!(
        MemoryStore::load(&trailing),
        Err(StoreError::Corrupt)
    ));
    // A state code outside 0..=2, or a huge length, is corrupt.
    let mut bad_state = good.clone();
    let state_at = good.len() - Envelope::MIN_LEN - 4 - 1;
    assert!(bad_state[state_at] <= 2);
    bad_state[state_at] = 9;
    assert!(matches!(
        MemoryStore::load(&bad_state),
        Err(StoreError::Corrupt)
    ));
    let mut huge = good;
    let len_at = huge.len() - Envelope::MIN_LEN - 4;
    huge[len_at..len_at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(matches!(MemoryStore::load(&huge), Err(StoreError::Corrupt)));
}
