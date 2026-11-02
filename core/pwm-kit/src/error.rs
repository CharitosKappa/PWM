use thiserror::Error;

/// Errors carry no input, so they are safe to log.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum KitError {
    #[error("invalid email")]
    InvalidEmail,
    #[error("invalid server address")]
    InvalidServer,
    #[error("invalid account id")]
    InvalidAccountId,
    #[error("invalid date")]
    InvalidDate,
    #[error("invalid Secret Key")]
    InvalidSecretKey,
    #[error("not an Emergency Kit code")]
    NotAKit,
    #[error("Emergency Kit code from a newer version")]
    UnsupportedVersion,
    #[error("the QR code could not be made")]
    Qr,
}
