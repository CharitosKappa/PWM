//! Windows: each value sealed with DPAPI for the current user (D66 §4), in
//! `keys/<name>.bin` under the app's local (never roaming) data folder.
//! DPAPI only protects the values on disk, as Android's Keystore does; no
//! crypto of ours runs here. This module and `system::windows` are the only
//! places with `unsafe` code: the Win32 calls themselves.
#![allow(unsafe_code)]

use std::path::PathBuf;

use pwm_client::Secret;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::PCWSTR;

use super::{KeyError, KeyStore, file_name, write_file};

pub struct DpapiKeys {
    dir: PathBuf,
}

impl DpapiKeys {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, name: &str) -> Result<PathBuf, KeyError> {
        Ok(self.dir.join(file_name(name)?))
    }
}

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, KeyError> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| KeyError)?,
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
fn protect(name: &str, data: &[u8]) -> Result<Vec<u8>, KeyError> {
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
    .map_err(|_| KeyError)?;
    Ok(take(out))
}

fn unprotect(name: &str, sealed: &[u8]) -> Result<Vec<u8>, KeyError> {
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
    .map_err(|_| KeyError)?;
    Ok(take(out))
}

impl KeyStore for DpapiKeys {
    fn secure(&self) -> bool {
        true
    }

    async fn get(&self, name: &str) -> Result<Option<Secret<String>>, KeyError> {
        let sealed = match std::fs::read(self.path(name)?) {
            Ok(sealed) => sealed,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(KeyError),
        };
        let bytes = unprotect(name, &sealed)?;
        let text = String::from_utf8(bytes).map_err(|_| KeyError)?;
        Ok(Some(Secret::new(text)))
    }

    async fn set(&self, name: &str, value: &str) -> Result<(), KeyError> {
        let sealed = protect(name, value.as_bytes())?;
        write_file(&self.path(name)?, &sealed)
    }

    async fn remove(&self, name: &str) -> Result<(), KeyError> {
        match std::fs::remove_file(self.path(name)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(KeyError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dpapi_round_trips_and_binds_the_name() {
        let dir = std::env::temp_dir().join(format!("pwm-dpapi-{}", std::process::id()));
        let keys = DpapiKeys::new(dir.clone());
        keys.set("secretKey", "A3-ABCDEF").await.unwrap();
        let file = std::fs::read(dir.join("secretKey.bin")).unwrap();
        assert!(!file.windows(9).any(|w| w == b"A3-ABCDEF"));
        assert_eq!(
            keys.get("secretKey")
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "A3-ABCDEF"
        );
        // Another key's file under this name doesn't open.
        std::fs::copy(dir.join("secretKey.bin"), dir.join("email.bin")).unwrap();
        assert!(keys.get("email").await.is_err());
        keys.remove("secretKey").await.unwrap();
        assert!(keys.get("secretKey").await.unwrap().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
