//! Settings (D66 §4): the server and the lock timeout, as `settings.json`
//! with `"v": 1`. Not secret, not synced. Checked as the extension checks
//! them (D34).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::protocol::Settings;

/// The hosted server: a placeholder until the product is named (D1).
pub const HOSTED_SERVER: &str = "https://api.pwm.invalid";
/// The lock timeout's choices, in minutes (D34: never "never").
pub const AUTO_LOCK_CHOICES: [u32; 5] = [1, 5, 15, 60, 480];
const DEFAULT_AUTO_LOCK: u32 = 15;
const VERSION: u8 = 1;

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: HOSTED_SERVER.to_owned(),
            auto_lock_minutes: DEFAULT_AUTO_LOCK,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    v: u8,
    server_url: String,
    auto_lock_minutes: u32,
    /// Added in M7.6, still version 1: a file without it reads as on.
    #[serde(default = "on")]
    leak_check: bool,
}

fn on() -> bool {
    true
}

/// Why new settings were refused: the screens' error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    InvalidServer,
    InvalidAutoLock,
}

impl Refused {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidServer => "invalid_server",
            Self::InvalidAutoLock => "invalid_auto_lock",
        }
    }
}

/// `https://host[:port]`, or `http://` to this computer only, with no path.
pub fn check(settings: &Settings) -> Result<Settings, Refused> {
    let url = settings.server_url.trim().trim_end_matches('/').to_owned();
    let valid = if let Some(rest) = url.strip_prefix("https://") {
        !rest.is_empty() && !rest.contains(['/', ' ', '\t', '\n'])
    } else if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split(':').next().unwrap_or("");
        let port_ok = rest
            .split_once(':')
            .is_none_or(|(_, p)| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        matches!(host, "localhost" | "127.0.0.1") && port_ok
    } else {
        false
    };
    if !valid {
        return Err(Refused::InvalidServer);
    }
    if !AUTO_LOCK_CHOICES.contains(&settings.auto_lock_minutes) {
        return Err(Refused::InvalidAutoLock);
    }
    Ok(Settings {
        server_url: url,
        auto_lock_minutes: settings.auto_lock_minutes,
    })
}

pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("settings.json"),
        }
    }

    /// The stored settings and the leak check's switch; the defaults when
    /// there are none or they don't read.
    pub fn load(&self) -> (Settings, bool) {
        let Ok(bytes) = std::fs::read(&self.path) else {
            return (Settings::default(), true);
        };
        match serde_json::from_slice::<File>(&bytes) {
            Ok(file) if file.v == VERSION => {
                let stored = Settings {
                    server_url: file.server_url,
                    auto_lock_minutes: file.auto_lock_minutes,
                };
                (check(&stored).unwrap_or_default(), file.leak_check)
            }
            _ => (Settings::default(), true),
        }
    }

    pub fn save(&self, settings: &Settings, leak_check: bool) -> Result<(), crate::keys::KeyError> {
        let file = File {
            v: VERSION,
            server_url: settings.server_url.clone(),
            auto_lock_minutes: settings.auto_lock_minutes,
            leak_check,
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|_| crate::keys::KeyError)?;
        crate::keys::write_file(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(url: &str, minutes: u32) -> Settings {
        Settings {
            server_url: url.to_owned(),
            auto_lock_minutes: minutes,
        }
    }

    #[test]
    fn servers_are_https_or_this_computer() {
        assert_eq!(
            check(&settings(" https://pwm.example.com/ ", 15))
                .unwrap()
                .server_url,
            "https://pwm.example.com"
        );
        assert!(check(&settings("http://localhost:8080", 15)).is_ok());
        assert!(check(&settings("http://127.0.0.1", 15)).is_ok());
        for bad in [
            "http://pwm.example.com",
            "https://pwm.example.com/path",
            "ftp://x",
            "https://",
            "http://localhost:",
            "http://localhost:80a",
        ] {
            assert_eq!(
                check(&settings(bad, 15)),
                Err(Refused::InvalidServer),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_lock_timeout_is_one_of_the_choices() {
        assert_eq!(
            check(&settings(HOSTED_SERVER, 0)),
            Err(Refused::InvalidAutoLock)
        );
        assert!(check(&settings(HOSTED_SERVER, 480)).is_ok());
    }

    #[test]
    fn the_file_is_versioned_and_bad_files_give_the_defaults() {
        let dir = std::env::temp_dir().join(format!("pwm-settings-{}", std::process::id()));
        let file = SettingsFile::new(&dir);
        assert_eq!(file.load(), (Settings::default(), true));
        let mine = settings("http://localhost:8080", 5);
        file.save(&mine, false).unwrap();
        assert_eq!(file.load(), (mine.clone(), false));
        let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"v\": 1"));
        // A file from before the leak check's switch (M7.4) reads as on.
        std::fs::write(
            dir.join("settings.json"),
            r#"{"v":1,"serverUrl":"http://localhost:8080","autoLockMinutes":5}"#,
        )
        .unwrap();
        assert_eq!(file.load(), (mine, true));
        std::fs::write(
            dir.join("settings.json"),
            r#"{"v":2,"serverUrl":"x","autoLockMinutes":5}"#,
        )
        .unwrap();
        assert_eq!(file.load(), (Settings::default(), true));
        std::fs::write(dir.join("settings.json"), "not json").unwrap();
        assert_eq!(file.load(), (Settings::default(), true));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
