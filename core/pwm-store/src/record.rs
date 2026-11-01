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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRecord {
    pub vault_id: Id,
    /// Vault key wrapped under the account `sym_key`.
    pub wrapped_key: Envelope,
    /// Highest server revision already pulled for this vault.
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
