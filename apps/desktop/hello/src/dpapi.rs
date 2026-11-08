//! DPAPI (D66 §4): values sealed for the current Windows user. It only
//! protects values on disk, as Android's Keystore does; no crypto of ours
//! runs here. This module has the crate's only `unsafe` code: the Win32
//! calls themselves.
#![allow(unsafe_code)]

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::PCWSTR;

/// DPAPI refused, or the value was sealed for another name or user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpapiError;

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, DpapiError> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| DpapiError)?,
        // DPAPI only reads the input.
        pbData: bytes.as_ptr().cast_mut(),
    })
}

/// Copies DPAPI's output out, wipes it and frees it.
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    let len = out.cbData as usize;
    // SAFETY: on success DPAPI returns `cbData` bytes at `pbData`, allocated
    // with LocalAlloc; they are copied, zeroed and freed exactly once here.
    unsafe {
        let bytes = std::slice::from_raw_parts(out.pbData, len).to_vec();
        std::ptr::write_bytes(out.pbData, 0, len);
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        bytes
    }
}

/// Seals `data` for this Windows user; `name` is DPAPI's extra entropy, so a
/// value can't be swapped for another's file.
pub fn protect(name: &str, data: &[u8]) -> Result<Vec<u8>, DpapiError> {
    let input = blob(data)?;
    let entropy = blob(name.as_bytes())?;
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the input blobs point at live slices for the call; `out` is
    // written by DPAPI and handed to `take`.
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .map_err(|_| DpapiError)?;
    Ok(take(out))
}

/// Opens [`protect`]'s output, for the same name and user only.
pub fn unprotect(name: &str, sealed: &[u8]) -> Result<Vec<u8>, DpapiError> {
    let input = blob(sealed)?;
    let entropy = blob(name.as_bytes())?;
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    }
    .map_err(|_| DpapiError)?;
    Ok(take(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_for_the_same_name_only() {
        let sealed = protect("secretKey", b"A3-ABCDEF").unwrap();
        assert!(!sealed.windows(9).any(|w| w == b"A3-ABCDEF"));
        assert_eq!(unprotect("secretKey", &sealed).unwrap(), b"A3-ABCDEF");
        assert_eq!(unprotect("email", &sealed).unwrap_err(), DpapiError);
    }
}
