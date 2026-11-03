use pwm_crypto::CryptoError;
use pwm_store::StoreError;
use pwm_vault::VaultError;
use thiserror::Error;

/// Sync errors. Network and server failures are not errors: they end a run
/// with [`Step::Wait`](crate::Step::Wait).
#[derive(Debug, Error)]
pub enum SyncError {
    #[error("a sync run is already in progress")]
    Busy,
    #[error("no sync run is waiting for a response")]
    NotRunning,
    #[error("unknown vault")]
    UnknownVault,
    #[error("the item is deleted")]
    ItemDeleted,
    /// Not one of the account's devices, or revoked already.
    #[error("unknown device")]
    UnknownDevice,
    /// The server refused a device request with this status.
    #[error("the server refused the request ({0})")]
    Refused(u16),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}
