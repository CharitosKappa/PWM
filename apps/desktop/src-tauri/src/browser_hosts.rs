//! "Allow browser extensions to unlock with Windows Hello" (D69 §4): the
//! desktop app registers the browser helper for this Windows user, as the
//! Mac app does for Touch ID (D48 §5). On: a manifest for Chromium browsers
//! and one for Firefox, under the app's local data folder, and a key per
//! browser under `HKEY_CURRENT_USER`. Off: the keys, the manifests, and
//! every enrolment the helper keeps. No administrator rights.

use std::path::PathBuf;

use pwm_browser_helper::{REGISTRY_KEYS, Store, credential_name, manifest};

use crate::backend::Failure;
use crate::hello::HelloKey;

type Result<T> = std::result::Result<T, Failure>;

/// A registry write Windows refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryError;

/// The registry under `HKEY_CURRENT_USER`, each key's default value.
pub trait Registry: Send + Sync + 'static {
    fn get(&self, path: &str) -> Option<String>;
    fn set(&self, path: &str, value: &str) -> std::result::Result<(), RegistryError>;
    fn remove(&self, path: &str);
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

    fn manifest_path(&self, firefox: bool) -> PathBuf {
        let form = if firefox { "firefox" } else { "chromium" };
        self.dir
            .join(format!("{}.{form}.json", pwm_browser_helper::HOST_NAME))
    }

    /// On when every browser's key names our manifest.
    pub fn is_on(&self) -> bool {
        REGISTRY_KEYS.iter().all(|(key, firefox)| {
            self.registry.get(key).as_deref() == self.manifest_path(*firefox).to_str()
        })
    }

    pub fn turn_on(&self) -> Result<()> {
        if !self.helper.is_file() {
            return Err(Failure::code("browser_helper_missing"));
        }
        let helper = self
            .helper
            .to_str()
            .ok_or_else(|| Failure::code("browser_helper_missing"))?;
        std::fs::create_dir_all(&self.dir).map_err(|_| Failure::code("storage"))?;
        for firefox in [false, true] {
            std::fs::write(self.manifest_path(firefox), manifest(firefox, helper))
                .map_err(|_| Failure::code("storage"))?;
        }
        for (key, firefox) in REGISTRY_KEYS {
            let path = self.manifest_path(*firefox);
            let value = path.to_str().ok_or_else(|| Failure::code("storage"))?;
            if self.registry.set(key, value).is_err() {
                self.turn_off(&crate::hello::NoHello);
                return Err(Failure::code("storage"));
            }
        }
        Ok(())
    }

    /// Removes our keys (only where they still name our manifests), the
    /// manifests, and every enrolment with its Windows Hello key.
    pub fn turn_off(&self, hello: &dyn HelloKey) {
        for (key, firefox) in REGISTRY_KEYS {
            if self.registry.get(key).as_deref() == self.manifest_path(*firefox).to_str() {
                self.registry.remove(key);
            }
        }
        for firefox in [false, true] {
            let _ = std::fs::remove_file(self.manifest_path(firefox));
        }
        for id in self.enrolments.ids() {
            hello.delete(&credential_name(&id));
            self.enrolments.delete(&id);
        }
    }
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

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use pwm_browser_helper::StoreError;

    use super::*;
    use crate::hello::FakeHello;

    #[derive(Default)]
    pub struct FakeRegistry(pub Mutex<HashMap<String, String>>);

    impl Registry for std::sync::Arc<FakeRegistry> {
        fn get(&self, path: &str) -> Option<String> {
            self.0.lock().unwrap().get(path).cloned()
        }
        fn set(&self, path: &str, value: &str) -> std::result::Result<(), RegistryError> {
            self.0
                .lock()
                .unwrap()
                .insert(path.to_owned(), value.to_owned());
            Ok(())
        }
        fn remove(&self, path: &str) {
            self.0.lock().unwrap().remove(path);
        }
    }

    #[derive(Default, Clone)]
    pub struct FakeEnrolments(pub std::sync::Arc<Mutex<HashMap<String, Vec<u8>>>>);

    impl Store for FakeEnrolments {
        fn get(&self, id: &str) -> Option<Vec<u8>> {
            self.0.lock().unwrap().get(id).cloned()
        }
        fn set(&self, id: &str, record: &[u8]) -> std::result::Result<(), StoreError> {
            self.0
                .lock()
                .unwrap()
                .insert(id.to_owned(), record.to_vec());
            Ok(())
        }
        fn delete(&self, id: &str) {
            self.0.lock().unwrap().remove(id);
        }
        fn ids(&self) -> Vec<String> {
            self.0.lock().unwrap().keys().cloned().collect()
        }
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pwm-hosts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Hosts over a temporary folder with a stand-in helper file.
    pub fn hosts(
        name: &str,
    ) -> (
        BrowserHosts,
        std::sync::Arc<FakeRegistry>,
        FakeEnrolments,
        PathBuf,
    ) {
        let dir = temp_dir(name);
        let helper = dir.join("pwm-browser-helper.exe");
        std::fs::write(&helper, b"").unwrap();
        let registry = std::sync::Arc::new(FakeRegistry::default());
        let enrolments = FakeEnrolments::default();
        let hosts = BrowserHosts::new(
            dir.join("NativeMessagingHosts"),
            helper,
            Box::new(registry.clone()),
            Box::new(enrolments.clone()),
        );
        (hosts, registry, enrolments, dir)
    }

    #[test]
    fn turning_on_writes_manifests_and_keys_for_each_browser() {
        let (hosts, registry, _, dir) = hosts("on");
        assert!(!hosts.is_on());
        hosts.turn_on().unwrap();
        assert!(hosts.is_on());
        let keys = registry.0.lock().unwrap().clone();
        assert_eq!(keys.len(), 3);
        let firefox = &keys[r"Software\Mozilla\NativeMessagingHosts\invalid.pwm.browser"];
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(firefox).unwrap()).unwrap();
        assert_eq!(manifest["allowed_extensions"][0], "pwm@pwm.invalid");
        assert_eq!(
            manifest["path"],
            dir.join("pwm-browser-helper.exe").to_str().unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn turning_off_removes_ours_and_forgets_every_enrolment() {
        let (hosts, registry, enrolments, dir) = hosts("off");
        hosts.turn_on().unwrap();
        // Another program's key with our name stays.
        registry.0.lock().unwrap().insert(
            r"Software\Microsoft\Edge\NativeMessagingHosts\invalid.pwm.browser".to_owned(),
            r"C:\Other\manifest.json".to_owned(),
        );
        let hello = FakeHello::default();
        let id = "0123456789abcdef0123456789abcdef";
        hello.create(&credential_name(id)).unwrap();
        enrolments.set(id, b"record").unwrap();

        hosts.turn_off(&hello);
        assert!(!hosts.is_on());
        assert_eq!(registry.0.lock().unwrap().len(), 1);
        assert!(enrolments.ids().is_empty());
        assert!(hello.state.lock().unwrap().keys.is_empty());
        assert!(
            !dir.join("NativeMessagingHosts")
                .join("invalid.pwm.browser.chromium.json")
                .exists()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_helper_is_said_plainly() {
        let (hosts, registry, _, dir) = hosts("missing");
        std::fs::remove_file(dir.join("pwm-browser-helper.exe")).unwrap();
        assert_eq!(hosts.turn_on().unwrap_err().0, "browser_helper_missing");
        assert!(registry.0.lock().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
