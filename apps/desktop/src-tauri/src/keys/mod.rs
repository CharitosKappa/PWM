//! The device's keys (D66 §4): the email, Secret Key, device id and tokens.
//! Windows seals each with DPAPI in the local data folder; Linux keeps them
//! in the Secret Service; a Linux desktop without one gets files only the
//! user can read, and the tokens are then sealed by the core (D34's model).

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;

use pwm_client::Secret;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

/// The key store failed: nothing was read or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("key store")]
pub struct KeyError;

/// The names of what the app keeps.
pub mod name {
    pub const EMAIL: &str = "email";
    pub const SECRET_KEY: &str = "secretKey";
    pub const DEVICE_ID: &str = "deviceId";
    /// The email of the account the device id belongs to.
    pub const DEVICE_ACCOUNT: &str = "deviceAccount";
    /// `{access, refresh}` as JSON, when the store is [`secure`](super::KeyStore::secure).
    pub const TOKENS: &str = "tokens";
    /// The tokens sealed under `sym_key` (base64), when it is not.
    pub const SEALED_TOKENS: &str = "sealedTokens";
}

pub trait KeyStore: Send + Sync + 'static {
    /// Whether the system protects the values (DPAPI, the Secret Service).
    /// When it doesn't, the tokens are kept sealed by the core.
    fn secure(&self) -> bool;
    fn get(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<Secret<String>>, KeyError>> + Send;
    fn set(&self, name: &str, value: &str) -> impl Future<Output = Result<(), KeyError>> + Send;
    fn remove(&self, name: &str) -> impl Future<Output = Result<(), KeyError>> + Send;
}

/// In memory only: tests.
#[derive(Default)]
pub struct MemoryKeys {
    values: Mutex<HashMap<String, Secret<String>>>,
    insecure: bool,
}

impl MemoryKeys {
    /// A store that says it isn't secure, as Linux without a Secret Service.
    pub fn insecure() -> Self {
        Self {
            insecure: true,
            ..Self::default()
        }
    }
}

impl KeyStore for MemoryKeys {
    fn secure(&self) -> bool {
        !self.insecure
    }

    async fn get(&self, name: &str) -> Result<Option<Secret<String>>, KeyError> {
        let values = self.values.lock().map_err(|_| KeyError)?;
        Ok(values
            .get(name)
            .map(|v| Secret::new(v.expose_secret().clone())))
    }

    async fn set(&self, name: &str, value: &str) -> Result<(), KeyError> {
        let mut values = self.values.lock().map_err(|_| KeyError)?;
        values.insert(name.to_owned(), Secret::new(value.to_owned()));
        Ok(())
    }

    async fn remove(&self, name: &str) -> Result<(), KeyError> {
        let mut values = self.values.lock().map_err(|_| KeyError)?;
        values.remove(name);
        Ok(())
    }
}

/// Writes `bytes` to `path` through a temporary file and a rename, so a
/// crash never leaves half a file. On Unix the file is the user's only.
pub fn write_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), KeyError> {
    use std::io::Write;
    let dir = path.parent().ok_or(KeyError)?;
    std::fs::create_dir_all(dir).map_err(|_| KeyError)?;
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).map_err(|_| KeyError)?;
    file.write_all(bytes).map_err(|_| KeyError)?;
    file.sync_all().map_err(|_| KeyError)?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|_| KeyError)
}

/// A key's file name: the names above are plain ASCII words.
pub fn file_name(name: &str) -> Result<String, KeyError> {
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(KeyError);
    }
    Ok(format!("{name}.bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_keys_keep_and_forget_values() {
        let keys = MemoryKeys::default();
        assert!(keys.secure());
        assert!(keys.get(name::EMAIL).await.unwrap().is_none());
        keys.set(name::EMAIL, "a@example.com").await.unwrap();
        assert_eq!(
            keys.get(name::EMAIL)
                .await
                .unwrap()
                .unwrap()
                .expose_secret(),
            "a@example.com"
        );
        keys.remove(name::EMAIL).await.unwrap();
        assert!(keys.get(name::EMAIL).await.unwrap().is_none());
    }

    #[test]
    fn key_names_are_plain_file_names() {
        assert_eq!(file_name("secretKey").unwrap(), "secretKey.bin");
        assert!(file_name("../x").is_err());
        assert!(file_name("").is_err());
    }

    #[test]
    fn files_are_written_whole() {
        let dir = std::env::temp_dir().join(format!("pwm-keys-{}", std::process::id()));
        let path = dir.join("a.bin");
        write_file(&path, b"one").unwrap();
        write_file(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!path.with_extension("tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
