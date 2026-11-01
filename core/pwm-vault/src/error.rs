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
    /// Valid data this app version cannot use yet (kept unchanged).
    #[error("not supported: {0}")]
    Unsupported(&'static str),
}
