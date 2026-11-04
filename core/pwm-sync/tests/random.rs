//! Randomized multi-device runs (D28): devices edit, delete and go offline
//! in random orders, then all sync. Some runs happen while locked (D50 §1).
//! They must end with identical vaults, and no saved content may disappear
//! unless its item was deleted.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use pwm_store::{SqliteStore, Store, SyncState};
use pwm_sync::SyncEngine;
use pwm_vault::{HISTORY_LIMIT, ItemId};

const T0: i64 = 1_700_000_000_000;
const DEVICES: usize = 3;
const ROUNDS: usize = 5;
const SEEDS: u64 = 40;

/// xorshift64*: deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn devices_converge_without_losing_edits() {
    for seed in 1..=SEEDS {
        run(seed);
    }
}

/// One sync without the keys, then unlocked again on the same cache.
fn sync_locked(device: &mut Engine, account: &Account, server: &mut FakeServer, now: i64) {
    let empty = SyncEngine::locked(SqliteStore::open_in_memory().unwrap(), server.new_device());
    let engine = std::mem::replace(device, empty.unwrap());
    let mut locked = SyncEngine::locked(engine.into_store(), server.new_device()).unwrap();
    sync(&mut locked, server, now);
    *device = SyncEngine::new(locked.into_store(), account.keyset(), server.new_device()).unwrap();
}

fn run(seed: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut devices: Vec<Engine> = (0..DEVICES).map(|_| account.device(&mut server)).collect();
    let vault = create_vault(&mut devices[0], &mut server);
    for device in &mut devices {
        sync(device, &mut server, T0);
    }

    let mut now = T0;
    let mut saved: BTreeMap<String, ItemId> = BTreeMap::new();
    let mut deleted: BTreeSet<ItemId> = BTreeSet::new();
    for round in 0..ROUNDS {
        for (d, device) in devices.iter_mut().enumerate() {
            now += 1_000;
            let title = format!("s{seed} r{round} d{d}");
            let items = device.items(&vault).unwrap();
            match rng.below(4) {
                0 | 1 if !items.is_empty() => {
                    let (id, _) = &items[rng.below(items.len())];
                    edit(device, &vault, id, &title, now);
                    saved.insert(title, *id);
                }
                2 if !items.is_empty() => {
                    let (id, _) = &items[rng.below(items.len())];
                    device.delete_item(&vault, id).unwrap();
                    deleted.insert(*id);
                }
                _ => {
                    let id = add(device, &vault, &title, now);
                    saved.insert(title, id);
                }
            }
        }
        // A random subset of devices is online this round, some locked.
        for device in &mut devices {
            match rng.below(3) {
                0 => {
                    now += 1_000;
                    sync(device, &mut server, now);
                }
                1 => {
                    now += 1_000;
                    sync_locked(device, &account, &mut server, now);
                }
                _ => {}
            }
        }
    }

    // Everyone comes online until nothing moves.
    let mut quiet = false;
    for _ in 0..10 {
        let mut moved = 0;
        for device in &mut devices {
            now += 1_000;
            let report = sync(device, &mut server, now);
            moved += report.pushed + report.pulled;
        }
        if moved == 0 {
            quiet = true;
            break;
        }
    }
    assert!(quiet, "seed {seed}: devices kept syncing");

    // Identical to the server, byte for byte, and nothing left to send.
    let fake = server.vault(&vault);
    for (d, device) in devices.iter().enumerate() {
        assert!(
            device.store().pending_items().unwrap().is_empty(),
            "seed {seed} device {d}"
        );
        let records = device.store().items(&vault).unwrap();
        assert_eq!(records.len(), fake.items.len(), "seed {seed} device {d}");
        for record in records {
            assert_eq!(record.state, SyncState::Synced);
            let (revision, _, blob) = &fake.items[record.item_id.as_bytes()];
            assert_eq!(record.revision, *revision, "seed {seed} device {d}");
            assert_eq!(&record.envelope.to_bytes(), blob, "seed {seed} device {d}");
        }
    }

    // Every saved title is still current, in history or in a copy, unless
    // its item was deleted or its history is full (D19 keeps 10 versions).
    let items = devices[0].items(&vault).unwrap();
    for (title, id) in &saved {
        let found = items
            .iter()
            .any(|(_, item)| item.title == *title || history_titles(item).contains(title));
        let history_full = items
            .iter()
            .any(|(i, item)| i == id && item.history().len() == HISTORY_LIMIT);
        assert!(
            found || deleted.contains(id) || history_full,
            "seed {seed}: lost {title}"
        );
    }
}
