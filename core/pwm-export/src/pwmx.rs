//! Password-protected `.pwmx` export (D24): the JSON export padded to 4 KiB
//! steps and sealed by `pwm_crypto::seal_export`.

use pwm_crypto::{KdfParams, open_export, seal_export};
use pwm_generator::{MIN_SIGNUP_SCORE, strength};

use crate::json::{read_json, write_json};
use crate::{Export, ExportError, ExportGrant, ExportVault};

const PAD_BLOCK: usize = 4096;

/// Refuses export passwords that score below 3 in zxcvbn (the signup rule).
pub fn check_export_password(password: &str) -> Result<(), ExportError> {
    if strength(password, &[]).score >= MIN_SIGNUP_SCORE {
        Ok(())
    } else {
        Err(ExportError::WeakPassword)
    }
}

/// The encrypted export: opens with `export_password` alone, without the
/// account. Check the password with [`check_export_password`] first: a weak
/// one is refused here after the grant is used up.
pub fn export_pwmx(
    grant: ExportGrant,
    now_ms: i64,
    vaults: &[ExportVault],
    export_password: &str,
) -> Result<Vec<u8>, ExportError> {
    check_export_password(export_password)?;
    grant.redeem(now_ms)?;
    let payload = write_json(now_ms, vaults, Some(PAD_BLOCK))?;
    Ok(seal_export(
        export_password,
        KdfParams::DEFAULT,
        payload.expose_secret(),
    )?)
}

/// Decrypts and parses a `.pwmx` file.
pub fn read_pwmx(file: &[u8], export_password: &str) -> Result<Export, ExportError> {
    let payload = open_export(export_password, file)?;
    read_json(unpad(payload.expose_secret())?)
}

fn unpad(buf: &[u8]) -> Result<&[u8], ExportError> {
    if buf.is_empty() || !buf.len().is_multiple_of(PAD_BLOCK) {
        return Err(ExportError::NotAnExport);
    }
    let end = buf
        .iter()
        .rposition(|&b| b != 0)
        .ok_or(ExportError::NotAnExport)?;
    if buf[end] != 0x80 {
        return Err(ExportError::NotAnExport);
    }
    Ok(&buf[..end])
}
