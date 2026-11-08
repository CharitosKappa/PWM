//! "Allow browser extensions to unlock with Windows Hello" (D69 §4), or
//! "with Linux login" (D71 §4): the desktop app registers the browser helper
//! for this user, as the Mac app does for Touch ID (D48 §5). The code lives in the helper's
//! library, so the uninstaller's `--uninstall` undoes it the same way
//! (D70 §5).

pub use pwm_browser_helper::hosts::{BrowserHosts, Hosts, HostsError, Registry, RegistryError};
pub use pwm_browser_helper::manifest_hosts::ManifestHosts;

use crate::backend::Failure;

impl From<HostsError> for Failure {
    fn from(error: HostsError) -> Self {
        Failure::code(match error {
            HostsError::HelperMissing => "browser_helper_missing",
            HostsError::Storage => "storage",
            HostsError::NoBrowser => "browser_not_found",
        })
    }
}
