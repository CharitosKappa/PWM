//! Re-authentication before any export (D24): only a master password that
//! opens the sealed keyset produces a grant, and each export consumes one.

use pwm_crypto::{Envelope, Id, KdfParams, SecretKey, unlock_account};

use crate::ExportError;

/// How long a grant stays valid after the master password was entered.
pub const GRANT_LIFETIME_MS: i64 = 5 * 60 * 1000;

/// Proof that the master password was just entered again. It is neither
/// `Clone` nor `Copy`, so every export needs a fresh one.
#[derive(Debug)]
pub struct ExportGrant {
    issued_at_ms: i64,
}

impl ExportGrant {
    /// Consumes the grant; fails if it is older than [`GRANT_LIFETIME_MS`] or
    /// the clock moved backwards since it was issued.
    pub(crate) fn redeem(self, now_ms: i64) -> Result<(), ExportError> {
        match now_ms.checked_sub(self.issued_at_ms) {
            Some(age) if (0..=GRANT_LIFETIME_MS).contains(&age) => Ok(()),
            _ => Err(ExportError::GrantExpired),
        }
    }
}

/// Checks the master password the same way unlocking does (re-derive the AUK,
/// open the sealed keyset) and issues a grant at `now_ms` (Unix ms, from the
/// caller's clock).
pub fn authorize_export(
    master_password: &str,
    secret_key: &SecretKey,
    account_id: &Id,
    salt: &[u8; 16],
    params: &KdfParams,
    sealed_keyset: &Envelope,
    now_ms: i64,
) -> Result<ExportGrant, ExportError> {
    // The opened keyset is dropped (and zeroized) right away.
    unlock_account(
        master_password,
        secret_key,
        account_id,
        salt,
        params,
        sealed_keyset,
    )?;
    Ok(ExportGrant {
        issued_at_ms: now_ms,
    })
}
