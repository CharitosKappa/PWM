//! Local encrypted cache (decision D20). The store keeps only what the server
//! also holds (envelopes, ids, revisions, KDF params) plus local sync state,
//! so a stolen locked device reveals nothing a server breach would not.
//! It does no crypto itself.

#![forbid(unsafe_code)]

mod error;
mod record;
#[cfg(feature = "sqlite")]
mod sqlite;

pub use error::StoreError;
pub use record::{AccountRecord, ItemRecord, SyncState, VaultRecord};
#[cfg(feature = "sqlite")]
pub use sqlite::{SCHEMA_VERSION, SqliteStore};

use pwm_crypto::Id;
use pwm_vault::ItemId;

/// Persistent storage for one signed-in account. Native platforms use
/// [`SqliteStore`]; the extension gets a browser backend later.
pub trait Store {
    fn account(&self) -> Result<Option<AccountRecord>, StoreError>;

    /// Stores or updates the account. A different account id is refused
    /// until [`clear`](Self::clear) is called.
    fn put_account(&mut self, account: &AccountRecord) -> Result<(), StoreError>;

    fn vaults(&self) -> Result<Vec<VaultRecord>, StoreError>;

    /// Inserts or replaces a vault.
    fn put_vault(&mut self, vault: &VaultRecord) -> Result<(), StoreError>;

    /// Removes a vault and all its items.
    fn delete_vault(&mut self, vault_id: &Id) -> Result<(), StoreError>;

    fn items(&self, vault_id: &Id) -> Result<Vec<ItemRecord>, StoreError>;

    fn item(&self, vault_id: &Id, item_id: &ItemId) -> Result<Option<ItemRecord>, StoreError>;

    /// Inserts or replaces items in one transaction: all are written or none.
    /// Each item's vault must already be stored.
    fn put_items(&mut self, items: &[ItemRecord]) -> Result<(), StoreError>;

    /// Removes an item for good. Removing a missing item is not an error.
    fn delete_item(&mut self, vault_id: &Id, item_id: &ItemId) -> Result<(), StoreError>;

    /// The server epoch seen at the last completed sync (D30); `None` before
    /// the first one.
    fn server_epoch(&self) -> Result<Option<Id>, StoreError>;

    fn set_server_epoch(&mut self, epoch: &Id) -> Result<(), StoreError>;

    /// Items with local changes not yet uploaded, across all vaults.
    fn pending_items(&self) -> Result<Vec<ItemRecord>, StoreError>;

    /// Deletes everything (sign out, device revoked).
    fn clear(&mut self) -> Result<(), StoreError>;
}
