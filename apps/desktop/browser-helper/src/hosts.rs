//! Registering the helper with the browsers for this Windows user (D69 §4),
//! and undoing it. The desktop's "Allow browser extensions to unlock with
//! Windows Hello" switch and the uninstaller (`--uninstall`, D70 §5) run
//! this same code.
//!
//! On: a manifest for Chromium browsers and one for Firefox, and a key per
//! browser under `HKEY_CURRENT_USER`. Off: the keys (only where they still
//! name our manifests), the manifests, and every enrolment with its Windows
//! Hello key. No administrator rights.

use std::path::{Path, PathBuf};

use pwm_hello::{HelloKey, NoHello};

use crate::{HOST_NAME, REGISTRY_KEYS, Store, credential_name, manifest};

/// A registry write Windows refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryError;

/// The registry under `HKEY_CURRENT_USER`, each key's default value.
pub trait Registry: Send + Sync + 'static {
    fn get(&self, path: &str) -> Option<String>;
    fn set(&self, path: &str, value: &str) -> Result<(), RegistryError>;
    fn remove(&self, path: &str);
}

/// Why turning it on failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostsError {
    /// `pwm-browser-helper` isn't next to the app.
    HelperMissing,
    /// A manifest or a registry key couldn't be written.
    Storage,
    /// Linux: none of the browsers it knows is installed (D71 §4).
    NoBrowser,
}

/// The desktop's switch: registering the helper with the browsers, and
/// undoing it. The registry on Windows ([`BrowserHosts`]), manifest files
/// on Linux (`ManifestHosts`).
pub trait Hosts: Send + Sync {
    fn is_on(&self) -> bool;
    fn turn_on(&self) -> Result<(), HostsError>;
    fn turn_off(&self, hello: &dyn HelloKey);
}

pub struct BrowserHosts {
    /// Where the manifests go (`%LOCALAPPDATA%\PWM\NativeMessagingHosts`).
    dir: PathBuf,
    /// `pwm-browser-helper.exe`, next to the app.
    helper: PathBuf,
    registry: Box<dyn Registry>,
    /// The helper's enrolments, to forget when turned off.
    enrolments: Box<dyn Store + Send + Sync>,
}

impl BrowserHosts {
    pub fn new(
        dir: PathBuf,
        helper: PathBuf,
        registry: Box<dyn Registry>,
        enrolments: Box<dyn Store + Send + Sync>,
    ) -> Self {
        Self {
            dir,
            helper,
            registry,
            enrolments,
        }
    }

    /// This Windows user's: the manifests under `%LOCALAPPDATA%\PWM`, the
    /// real registry and the DPAPI enrolment files.
    #[cfg(windows)]
    pub fn for_this_user(helper: PathBuf) -> Option<Self> {
        let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("PWM");
        Some(Self::new(
            local.join("NativeMessagingHosts"),
            helper,
            Box::new(WindowsRegistry),
            Box::new(crate::store::FileStore::new()?),
        ))
    }

    fn manifest_path(&self, firefox: bool) -> PathBuf {
        let form = if firefox { "firefox" } else { "chromium" };
        self.dir.join(format!("{HOST_NAME}.{form}.json"))
    }

    fn names_ours(&self, key: &str, firefox: bool) -> bool {
        self.registry.get(key).as_deref() == self.manifest_path(firefox).to_str()
    }
}

impl Hosts for BrowserHosts {
    /// On when every browser's key names our manifest.
    fn is_on(&self) -> bool {
        REGISTRY_KEYS
            .iter()
            .all(|(key, firefox)| self.names_ours(key, *firefox))
    }

    fn turn_on(&self) -> Result<(), HostsError> {
        if !self.helper.is_file() {
            return Err(HostsError::HelperMissing);
        }
        let helper = self.helper.to_str().ok_or(HostsError::HelperMissing)?;
        std::fs::create_dir_all(&self.dir).map_err(|_| HostsError::Storage)?;
        for firefox in [false, true] {
            std::fs::write(self.manifest_path(firefox), manifest(firefox, helper))
                .map_err(|_| HostsError::Storage)?;
        }
        for (key, firefox) in REGISTRY_KEYS {
            let path = self.manifest_path(*firefox);
            let value = path.to_str().ok_or(HostsError::Storage)?;
            if self.registry.set(key, value).is_err() {
                self.turn_off(&NoHello);
                return Err(HostsError::Storage);
            }
        }
        Ok(())
    }

    /// Removes our keys (only where they still name our manifests), the
    /// manifests and their folder once empty, and every enrolment with its
    /// Windows Hello key. Deleting a key needs no prompt.
    fn turn_off(&self, hello: &dyn HelloKey) {
        for (key, firefox) in REGISTRY_KEYS {
            if self.names_ours(key, *firefox) {
                self.registry.remove(key);
            }
        }
        for firefox in [false, true] {
            let _ = std::fs::remove_file(self.manifest_path(firefox));
        }
        remove_if_empty(&self.dir);
        for id in self.enrolments.ids() {
            hello.delete(&credential_name(&id));
            self.enrolments.delete(&id);
        }
    }
}

/// Only an empty folder goes: anything else in it stays.
fn remove_if_empty(dir: &Path) {
    let _ = std::fs::remove_dir(dir);
}

/// Whether the uninstaller started the helper (D70 §5). Browsers pass an
/// origin, or a manifest path and an extension id, never this.
pub fn is_uninstall(args: &[String]) -> bool {
    matches!(args, [_, flag] if flag == "--uninstall")
}

#[cfg(windows)]
pub use windows_registry_impl::WindowsRegistry;

#[cfg(windows)]
mod windows_registry_impl {
    use windows_registry::CURRENT_USER;

    use super::{Registry, RegistryError};

    pub struct WindowsRegistry;

    impl Registry for WindowsRegistry {
        fn get(&self, path: &str) -> Option<String> {
            CURRENT_USER.open(path).ok()?.get_string("").ok()
        }

        fn set(&self, path: &str, value: &str) -> Result<(), RegistryError> {
            CURRENT_USER
                .create(path)
                .and_then(|key| key.set_string("", value))
                .map_err(|_| RegistryError)
        }

        fn remove(&self, path: &str) {
            let _ = CURRENT_USER.remove_tree(path);
        }
    }
}
