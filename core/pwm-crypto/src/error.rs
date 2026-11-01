//! Error type for all `pwm-crypto` operations.

/// Errors carry no secret material and deliberately do not say *why*
/// authentication failed (wrong key, wrong AAD and tampering look the same).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("unsupported envelope version {0}")]
    UnsupportedVersion(u8),
    #[error("envelope is truncated")]
    Truncated,
    #[error("decryption failed")]
    DecryptionFailed,
    #[error("encryption failed")]
    EncryptionFailed,
    #[error("the OS random number generator failed")]
    Rng,
    #[error("KDF parameters are outside the allowed range")]
    KdfParamsRejected,
    #[error("key derivation failed")]
    Kdf,
    #[error("Secret Key is not in the expected format")]
    SecretKeyFormat,
    #[error("Secret Key check character does not match (likely a typo)")]
    SecretKeyChecksum,
    #[error("password is empty or contains control characters")]
    PasswordRejected,
    #[error("signature verification failed")]
    SignatureInvalid,
    #[error("public key is invalid or of small order")]
    InvalidPublicKey,
    #[error("not a PWM export file")]
    NotExportFile,
}
