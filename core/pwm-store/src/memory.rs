//! In-memory cache with a versioned snapshot (decision D31), for the browser
//! extension: IndexedDB is asynchronous and the [`Store`] trait is not, so
//! the extension keeps this store in memory and persists [`snapshot`]s.
//!
//! Snapshot v2, all integers big-endian:
//!
//! ```text
//! "PWMS" || v:u8 (= 2)
//! || has_account:u8 [account_id:16 salt:16 m:u32 t:u32 p:u32 len:u32 sealed_keyset]
//! || has_epoch:u8 [epoch:16]
//! || has_plan:u8 [len:u8 plan has_until:u8 [until_ms:i64]]
//! || vaults:u32 { vault_id:16 cursor:u64 len:u32 wrapped_key }
//! || items:u32 { vault_id:16 item_id:16 revision:u64 state:u8 len:u32 envelope }
//! ```
//!
//! v1 (D31) is the same without `has_plan` (D73); it loads with no plan.
//!
//! Vaults and items are sorted by id, so equal contents give equal bytes. It
//! holds only what the SQLite cache holds: ids, revisions, cursors, KDF
//! params and ciphertext (D20).
//!
//! [`snapshot`]: MemoryStore::snapshot

use std::collections::BTreeMap;

use pwm_crypto::{Envelope, Id, KdfParams};
use pwm_vault::ItemId;

use crate::{AccountRecord, ItemRecord, PlanRecord, Store, StoreError, SyncState, VaultRecord};

/// Snapshot format this build writes and the newest it can load.
pub const SNAPSHOT_VERSION: u8 = 2;
const MAGIC: &[u8; 4] = b"PWMS";

#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    account: Option<AccountRecord>,
    epoch: Option<Id>,
    plan: Option<PlanRecord>,
    vaults: BTreeMap<Id, VaultRecord>,
    items: BTreeMap<(Id, ItemId), ItemRecord>,
}

/// Values above `i64::MAX` are refused, as SQLite stores them as integers.
fn fits_sqlite(value: u64, what: &'static str) -> Result<(), StoreError> {
    if i64::try_from(value).is_ok() {
        Ok(())
    } else {
        Err(StoreError::Invalid(what))
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The whole store in the snapshot format.
    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(SNAPSHOT_VERSION);
        match &self.account {
            Some(a) => {
                out.push(1);
                out.extend_from_slice(&a.account_id);
                out.extend_from_slice(&a.salt);
                for n in [
                    a.kdf_params.memory_kib,
                    a.kdf_params.iterations,
                    a.kdf_params.parallelism,
                ] {
                    out.extend_from_slice(&n.to_be_bytes());
                }
                put_bytes(&mut out, &a.sealed_keyset.to_bytes());
            }
            None => out.push(0),
        }
        match &self.epoch {
            Some(epoch) => {
                out.push(1);
                out.extend_from_slice(epoch);
            }
            None => out.push(0),
        }
        match &self.plan {
            Some(plan) => {
                out.push(1);
                // At most 32 bytes (`PlanRecord::check`).
                out.push(u8::try_from(plan.plan.len()).unwrap_or(0));
                out.extend_from_slice(plan.plan.as_bytes());
                match plan.until_ms {
                    Some(until) => {
                        out.push(1);
                        out.extend_from_slice(&until.to_be_bytes());
                    }
                    None => out.push(0),
                }
            }
            None => out.push(0),
        }
        put_count(&mut out, self.vaults.len());
        for v in self.vaults.values() {
            out.extend_from_slice(&v.vault_id);
            out.extend_from_slice(&v.sync_cursor.to_be_bytes());
            put_bytes(&mut out, &v.wrapped_key.to_bytes());
        }
        put_count(&mut out, self.items.len());
        for i in self.items.values() {
            out.extend_from_slice(&i.vault_id);
            out.extend_from_slice(i.item_id.as_bytes());
            out.extend_from_slice(&i.revision.to_be_bytes());
            out.push(state_code(i.state));
            put_bytes(&mut out, &i.envelope.to_bytes());
        }
        out
    }

    /// Loads a [`snapshot`](Self::snapshot). Anything malformed, truncated or
    /// followed by extra bytes is refused as corrupt.
    pub fn load(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() < MAGIC.len() + 1 || &bytes[..4] != MAGIC {
            return Err(StoreError::NotAStore);
        }
        let version = bytes[4];
        if version > SNAPSHOT_VERSION {
            return Err(StoreError::NewerSchema {
                found: u32::from(version),
                supported: u32::from(SNAPSHOT_VERSION),
            });
        }
        let mut r = Reader(&bytes[5..]);
        let mut store = Self::new();
        if r.flag()? {
            store.account = Some(AccountRecord {
                account_id: r.array()?,
                salt: r.array()?,
                kdf_params: KdfParams {
                    memory_kib: r.u32()?,
                    iterations: r.u32()?,
                    parallelism: r.u32()?,
                },
                sealed_keyset: r.envelope()?,
            });
        }
        if r.flag()? {
            store.epoch = Some(r.array()?);
        }
        if version >= 2 && r.flag()? {
            let len = usize::from(r.u8()?);
            let plan = String::from_utf8(r.take(len)?.to_vec()).map_err(|_| StoreError::Corrupt)?;
            let until_ms = if r.flag()? {
                Some(i64::from_be_bytes(r.array()?))
            } else {
                None
            };
            let plan = PlanRecord { plan, until_ms };
            plan.check().map_err(|_| StoreError::Corrupt)?;
            store.plan = Some(plan);
        }
        for _ in 0..r.u32()? {
            let vault = VaultRecord {
                vault_id: r.array()?,
                sync_cursor: r.u64()?,
                wrapped_key: r.envelope()?,
            };
            fits_sqlite(vault.sync_cursor, "sync cursor").map_err(|_| StoreError::Corrupt)?;
            if store.vaults.insert(vault.vault_id, vault).is_some() {
                return Err(StoreError::Corrupt);
            }
        }
        for _ in 0..r.u32()? {
            let item = ItemRecord {
                vault_id: r.array()?,
                item_id: ItemId::from_bytes(r.array()?),
                revision: r.u64()?,
                state: state_from_code(r.u8()?).ok_or(StoreError::Corrupt)?,
                envelope: r.envelope()?,
            };
            fits_sqlite(item.revision, "revision").map_err(|_| StoreError::Corrupt)?;
            if !store.vaults.contains_key(&item.vault_id)
                || store
                    .items
                    .insert((item.vault_id, item.item_id), item)
                    .is_some()
            {
                return Err(StoreError::Corrupt);
            }
        }
        if !r.0.is_empty() {
            return Err(StoreError::Corrupt);
        }
        Ok(store)
    }
}

impl Store for MemoryStore {
    fn account(&self) -> Result<Option<AccountRecord>, StoreError> {
        Ok(self.account.clone())
    }

    fn put_account(&mut self, account: &AccountRecord) -> Result<(), StoreError> {
        if self
            .account
            .as_ref()
            .is_some_and(|a| a.account_id != account.account_id)
        {
            return Err(StoreError::Invalid(
                "a different account is stored; clear the store first",
            ));
        }
        self.account = Some(account.clone());
        Ok(())
    }

    fn vaults(&self) -> Result<Vec<VaultRecord>, StoreError> {
        Ok(self.vaults.values().cloned().collect())
    }

    fn put_vault(&mut self, vault: &VaultRecord) -> Result<(), StoreError> {
        fits_sqlite(vault.sync_cursor, "sync cursor above i64::MAX")?;
        self.vaults.insert(vault.vault_id, vault.clone());
        Ok(())
    }

    fn delete_vault(&mut self, vault_id: &Id) -> Result<(), StoreError> {
        self.vaults.remove(vault_id);
        self.items.retain(|(v, _), _| v != vault_id);
        Ok(())
    }

    fn items(&self, vault_id: &Id) -> Result<Vec<ItemRecord>, StoreError> {
        Ok(self
            .items
            .values()
            .filter(|i| i.vault_id == *vault_id)
            .cloned()
            .collect())
    }

    fn item(&self, vault_id: &Id, item_id: &ItemId) -> Result<Option<ItemRecord>, StoreError> {
        Ok(self.items.get(&(*vault_id, *item_id)).cloned())
    }

    fn put_items(&mut self, items: &[ItemRecord]) -> Result<(), StoreError> {
        // Check everything first: all are written or none.
        for item in items {
            fits_sqlite(item.revision, "revision above i64::MAX")?;
            if !self.vaults.contains_key(&item.vault_id) {
                return Err(StoreError::Invalid("item's vault is not stored"));
            }
        }
        for item in items {
            self.items
                .insert((item.vault_id, item.item_id), item.clone());
        }
        Ok(())
    }

    fn delete_item(&mut self, vault_id: &Id, item_id: &ItemId) -> Result<(), StoreError> {
        self.items.remove(&(*vault_id, *item_id));
        Ok(())
    }

    fn server_epoch(&self) -> Result<Option<Id>, StoreError> {
        Ok(self.epoch)
    }

    fn set_server_epoch(&mut self, epoch: &Id) -> Result<(), StoreError> {
        self.epoch = Some(*epoch);
        Ok(())
    }

    fn plan(&self) -> Result<Option<PlanRecord>, StoreError> {
        Ok(self.plan.clone())
    }

    fn set_plan(&mut self, plan: &PlanRecord) -> Result<(), StoreError> {
        plan.check()?;
        self.plan = Some(plan.clone());
        Ok(())
    }

    fn pending_items(&self) -> Result<Vec<ItemRecord>, StoreError> {
        Ok(self
            .items
            .values()
            .filter(|i| i.state != SyncState::Synced)
            .cloned()
            .collect())
    }

    fn clear(&mut self) -> Result<(), StoreError> {
        *self = Self::new();
        Ok(())
    }
}

/// The same codes as the SQLite cache's `items.state`.
fn state_code(state: SyncState) -> u8 {
    match state {
        SyncState::Synced => 0,
        SyncState::Modified => 1,
        SyncState::Deleted => 2,
    }
}

fn state_from_code(code: u8) -> Option<SyncState> {
    match code {
        0 => Some(SyncState::Synced),
        1 => Some(SyncState::Modified),
        2 => Some(SyncState::Deleted),
        _ => None,
    }
}

fn put_count(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes());
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_count(out, bytes.len());
    out.extend_from_slice(bytes);
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], StoreError> {
        if self.0.len() < n {
            return Err(StoreError::Corrupt);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StoreError> {
        self.take(N)?.try_into().map_err(|_| StoreError::Corrupt)
    }

    fn u8(&mut self) -> Result<u8, StoreError> {
        Ok(self.array::<1>()?[0])
    }

    fn flag(&mut self) -> Result<bool, StoreError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(StoreError::Corrupt),
        }
    }

    fn u32(&mut self) -> Result<u32, StoreError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, StoreError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn envelope(&mut self) -> Result<Envelope, StoreError> {
        let len = usize::try_from(self.u32()?).map_err(|_| StoreError::Corrupt)?;
        Envelope::from_bytes(self.take(len)?).map_err(|_| StoreError::Corrupt)
    }
}
