//! Checking what the user entered as a login's one-time code (D23 §3).

use pwm_crypto::Secret;

use crate::ClientError;

/// The value to store in a login's `totp` for what the user pasted or
/// scanned: a URI as entered, or a bare base32 secret as an `otpauth://`
/// URI named after `title`. Anything else is `invalid_totp`.
pub fn normalize_totp(input: &str, title: &str) -> Result<Secret<String>, ClientError> {
    pwm_vault::normalize_totp(input, title).map_err(|_| ClientError::InvalidTotp)
}
