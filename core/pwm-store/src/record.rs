use pwm_crypto::{Envelope, Id, KdfParams};
use pwm_vault::ItemId;

/// What a device needs to unlock the account; the same values the server
/// returns at login. The Secret Key is never stored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub account_id: Id,
    pub salt: [u8; 16],
    pub kdf_params: KdfParams,
    /// Account keyset sealed under the AUK.
    pub sealed_keyset: Envelope,
}

/// The account's plan as the server last said (D73 §2): `free`, `premium`,
/// `family` or `teams`, and when it stops counting if nothing renews (Unix
/// ms); `None` when it has no end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRecord {
    pub plan: String,
    pub until_ms: Option<i64>,
}

impl PlanRecord {
    /// A plan name is 1 to 32 bytes, as both caches can hold.
    pub(crate) fn check(&self) -> Result<(), crate::StoreError> {
        if (1..=32).contains(&self.plan.len()) {
            Ok(())
        } else {
            Err(crate::StoreError::Invalid(
                "plan name must be 1 to 32 bytes",
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRecord {
    pub vault_id: Id,
    /// Vault key wrapped under the account `sym_key`.
    pub wrapped_key: Envelope,
    /// The vault's change counter (`seq`, D27) pulled up to: every change
    /// at or below it is in the cache.
    pub sync_cursor: u64,
}

/// Whether an item has local changes the server has not seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    Synced,
    /// Edited locally; upload pending.
    Modified,
    /// Deleted locally; tombstone upload pending. Hidden from the UI.
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRecord {
    pub vault_id: Id,
    pub item_id: ItemId,
    /// Revision bound into the envelope's AAD.
    pub revision: u64,
    pub envelope: Envelope,
    pub state: SyncState,
}
