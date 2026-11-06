//! Linux: the Secret Service (GNOME Keyring, KWallet) through `oo7`, which
//! inside Flatpak uses the Secret portal (D66 §4). Without one, files only
//! the user can read, and the store says it isn't secure, so the tokens are
//! sealed by the core.

use std::path::PathBuf;

use pwm_client::Secret;

use super::{KeyError, KeyStore, file_name, write_file};

const APPLICATION: &str = "pwm-desktop";

/// The Secret Service, or files when there is none.
pub enum LinuxKeys {
    Service(oo7::Keyring),
    Files(PathBuf),
}

impl LinuxKeys {
    /// The Secret Service when it answers, otherwise files in `dir`.
    pub async fn open(dir: PathBuf) -> Self {
        match oo7::Keyring::new().await {
            Ok(keyring) if keyring.unlock().await.is_ok() => Self::Service(keyring),
            _ => Self::Files(dir),
        }
    }
}

fn attributes(name: &str) -> [(&'static str, &str); 2] {
    [("application", APPLICATION), ("name", name)]
}

impl KeyStore for LinuxKeys {
    fn secure(&self) -> bool {
        matches!(self, Self::Service(_))
    }

    async fn get(&self, name: &str) -> Result<Option<Secret<String>>, KeyError> {
        match self {
            Self::Service(keyring) => {
                let items = keyring
                    .search_items(&attributes(name))
                    .await
                    .map_err(|_| KeyError)?;
                let Some(item) = items.first() else {
                    return Ok(None);
                };
                let secret = item.secret().await.map_err(|_| KeyError)?;
                let text = std::str::from_utf8(secret.as_bytes()).map_err(|_| KeyError)?;
                Ok(Some(Secret::new(text.to_owned())))
            }
            Self::Files(dir) => match std::fs::read(dir.join(file_name(name)?)) {
                Ok(bytes) => Ok(Some(Secret::new(
                    String::from_utf8(bytes).map_err(|_| KeyError)?,
                ))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(KeyError),
            },
        }
    }

    async fn set(&self, name: &str, value: &str) -> Result<(), KeyError> {
        match self {
            Self::Service(keyring) => keyring
                .create_item(
                    &format!("PWM: {name}"),
                    &attributes(name),
                    oo7::Secret::text(value),
                    true,
                )
                .await
                .map_err(|_| KeyError),
            Self::Files(dir) => write_file(&dir.join(file_name(name)?), value.as_bytes()),
        }
    }

    async fn remove(&self, name: &str) -> Result<(), KeyError> {
        match self {
            Self::Service(keyring) => keyring
                .delete(&attributes(name))
                .await
                .map_err(|_| KeyError),
            Self::Files(dir) => match std::fs::remove_file(dir.join(file_name(name)?)) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(KeyError),
            },
        }
    }
}
