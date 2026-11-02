//! Error type for `pwm-vault`.

use pwm_crypto::CryptoError;

/// Errors never include item content. Parser messages are dropped on purpose:
/// they can quote the offending value, which may be a password.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("item data is malformed")]
    InvalidItem,
    #[error("item is read-only in this app version")]
    ReadOnly,
    #[error("invalid item: {0}")]
    Invalid(&'static str),
    /// Sealed, the item is over [`MAX_SEALED_ITEM_BYTES`](crate::MAX_SEALED_ITEM_BYTES)
    /// even without history (D29).
    #[error("item is too large")]
    TooLarge,
    /// Valid data this app version cannot use yet (kept unchanged).
    #[error("not supported: {0}")]
    Unsupported(&'static str),
}
