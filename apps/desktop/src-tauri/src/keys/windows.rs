//! Windows: each value sealed with DPAPI for the current user (D66 §4), in
//! `keys/<name>.bin` under the app's local (never roaming) data folder.
//! DPAPI only protects the values on disk, as Android's Keystore does; no
//! crypto of ours runs here. The Win32 calls are in `pwm_hello::dpapi`.

use std::path::PathBuf;

use pwm_client::Secret;

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

fn protect(name: &str, data: &[u8]) -> Result<Vec<u8>, KeyError> {
    pwm_hello::dpapi::protect(name, data).map_err(|_| KeyError)
}

fn unprotect(name: &str, sealed: &[u8]) -> Result<Vec<u8>, KeyError> {
    pwm_hello::dpapi::unprotect(name, sealed).map_err(|_| KeyError)
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
