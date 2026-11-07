//! Registering the helper with the browsers on Linux (D71 §4): a manifest
//! in each installed browser's folder under the user's home. Chromium
//! browsers read `NativeMessagingHosts/` in their profile folder, Firefox
//! `~/.mozilla/native-messaging-hosts/`. Only browsers whose folder exists
//! get one; turning it on again picks up a browser installed since.

use std::path::{Path, PathBuf};

use pwm_hello::HelloKey;

use crate::hosts::{Hosts, HostsError};
use crate::{HOST_NAME, Store, credential_name, manifest};

/// Each browser's profile folder under the home, and whether it reads
/// Firefox's manifest form. Snap and Flatpak browsers can't start a helper
/// on the computer (D71 §4).
pub const LINUX_BROWSERS: &[(&str, bool)] = &[
    (".config/google-chrome", false),
    (".config/chromium", false),
    (".config/microsoft-edge", false),
    (".config/BraveSoftware/Brave-Browser", false),
    (".config/vivaldi", false),
    (".mozilla", true),
];

pub struct ManifestHosts {
    home: PathBuf,
    /// `pwm-browser-helper`, next to the app.
    helper: PathBuf,
    /// The helper's enrolments, to forget when turned off.
    enrolments: Box<dyn Store + Send + Sync>,
}

impl ManifestHosts {
    pub fn new(home: PathBuf, helper: PathBuf, enrolments: Box<dyn Store + Send + Sync>) -> Self {
        Self {
            home,
            helper,
            enrolments,
        }
    }

    /// This user's: `$HOME` and the enrolment files.
    #[cfg(not(windows))]
    pub fn for_this_user(helper: PathBuf) -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(Self::new(
            home,
            helper,
            Box::new(crate::store::FileStore::new()?),
        ))
    }

    fn manifest_path(profile: &Path, firefox: bool) -> PathBuf {
        let folder = if firefox {
            "native-messaging-hosts"
        } else {
            "NativeMessagingHosts"
        };
        profile.join(folder).join(format!("{HOST_NAME}.json"))
    }

    /// The installed browsers' manifest paths.
    fn installed(&self) -> Vec<(PathBuf, bool)> {
        LINUX_BROWSERS
            .iter()
            .map(|(folder, firefox)| (self.home.join(folder), *firefox))
            .filter(|(profile, _)| profile.is_dir())
            .map(|(profile, firefox)| (Self::manifest_path(&profile, firefox), firefox))
            .collect()
    }

    /// Whether the manifest at `path` names our helper.
    fn names_ours(&self, path: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            return false;
        };
        json["path"].as_str() == self.helper.to_str()
    }
}

impl Hosts for ManifestHosts {
    /// On when some installed browser has our manifest.
    fn is_on(&self) -> bool {
        self.installed()
            .iter()
            .any(|(path, _)| self.names_ours(path))
    }

    fn turn_on(&self) -> Result<(), HostsError> {
        if !self.helper.is_file() {
            return Err(HostsError::HelperMissing);
        }
        let helper = self.helper.to_str().ok_or(HostsError::HelperMissing)?;
        let installed = self.installed();
        if installed.is_empty() {
            return Err(HostsError::NoBrowser);
        }
        for (path, firefox) in installed {
            let folder = path.parent().ok_or(HostsError::Storage)?;
            std::fs::create_dir_all(folder).map_err(|_| HostsError::Storage)?;
            std::fs::write(&path, manifest(firefox, helper)).map_err(|_| HostsError::Storage)?;
        }
        Ok(())
    }

    /// Removes our manifests (only those naming our helper) and every
    /// enrolment. The key holder's secrets stay: without what the user's
    /// side kept they open nothing (D71 §1).
    fn turn_off(&self, hello: &dyn HelloKey) {
        for (path, _) in self.installed() {
            if self.names_ours(&path) {
                let _ = std::fs::remove_file(&path);
            }
        }
        for id in self.enrolments.ids() {
            hello.delete(&credential_name(&id));
            self.enrolments.delete(&id);
        }
    }
}
