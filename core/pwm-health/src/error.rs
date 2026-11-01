use pwm_crypto::CryptoError;
use thiserror::Error;

/// Health check errors. Messages never contain passwords or hashes.
#[derive(Debug, Error)]
pub enum HealthError {
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    /// A range response was not `SUFFIX:COUNT` lines.
    #[error("malformed HIBP range response")]
    BadResponse,
    /// A response was added for a prefix this check never asked for.
    #[error("HIBP response for a prefix that was not requested")]
    UnexpectedPrefix,
    /// The platform could not fetch a range (offline, HTTP error, ...).
    #[error("HIBP request failed: {0}")]
    Fetch(String),
}
