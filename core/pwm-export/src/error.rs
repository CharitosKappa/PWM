//! Error type for `pwm-export`.

use pwm_crypto::CryptoError;
use pwm_vault::VaultError;

/// Errors never include file content: a parser message could quote a password.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("enter your master password again to export")]
    GrantExpired,
    #[error("the export password is too easy to guess")]
    WeakPassword,
    #[error("not a PWM export")]
    NotAnExport,
    #[error("export format version {0} is not supported")]
    UnsupportedVersion(u64),
    #[error("CSV row {row} is invalid: {reason}")]
    InvalidCsv { row: usize, reason: &'static str },
}
