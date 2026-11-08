//! Each enrolment's record (the challenge and the sealed K, never K).
//!
//! - **Windows:** `%LOCALAPPDATA%\PWM\browser-unlock\<id>.bin`, protected
//!   with DPAPI for this Windows user, with the id as DPAPI's entropy (D69 §2).
//! - **Linux:** `$XDG_DATA_HOME/pwm/browser-unlock/<id>.bin` (or under
//!   `~/.local/share`), which only this user can read (D71). Without the key
//!   holder's secret, which only root reads, a record opens nothing.

use std::path::{Path, PathBuf};

use crate::{Store, StoreError};

#[cfg(windows)]
fn protect(id: &str, record: &[u8]) -> Result<Vec<u8>, StoreError> {
    pwm_hello::dpapi::protect(id, record).map_err(|_| StoreError)
}

#[cfg(windows)]
fn unprotect(id: &str, sealed: &[u8]) -> Option<Vec<u8>> {
    pwm_hello::dpapi::unprotect(id, sealed).ok()
}

#[cfg(not(windows))]
fn protect(_: &str, record: &[u8]) -> Result<Vec<u8>, StoreError> {
    Ok(record.to_vec())
}

#[cfg(not(windows))]
fn unprotect(_: &str, sealed: &[u8]) -> Option<Vec<u8>> {
    Some(sealed.to_vec())
}

/// A file only this user can read, made with that mode from the start.
fn write_private(path: &Path, contents: &[u8]) -> Result<(), StoreError> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| StoreError)?;
    file.write_all(contents).map_err(|_| StoreError)
}

pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    #[cfg(windows)]
    pub fn new() -> Option<Self> {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(Self::at(
            PathBuf::from(base).join("PWM").join("browser-unlock"),
        ))
    }

    #[cfg(not(windows))]
    pub fn new() -> Option<Self> {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/share")))?;
        Some(Self::at(data.join("pwm").join("browser-unlock")))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The folder goes once no enrolment is left in it (D70 §5).
    pub fn remove_if_empty(&self) {
        let _ = std::fs::remove_dir(&self.dir);
    }

    /// Ids are 32 hex digits (`enrolment`), so always a plain file name.
    fn path(&self, id: &str) -> Option<PathBuf> {
        id.bytes()
            .all(|b| b.is_ascii_hexdigit())
            .then(|| self.dir.join(format!("{id}.bin")))
    }
}

impl Store for FileStore {
    fn get(&self, id: &str) -> Option<Vec<u8>> {
        let sealed = std::fs::read(self.path(id)?).ok()?;
        unprotect(id, &sealed)
    }

    fn set(&self, id: &str, record: &[u8]) -> Result<(), StoreError> {
        let path = self.path(id).ok_or(StoreError)?;
        let sealed = protect(id, record)?;
        std::fs::create_dir_all(&self.dir).map_err(|_| StoreError)?;
        let temporary = path.with_extension("tmp");
        write_private(&temporary, &sealed)?;
        std::fs::rename(&temporary, &path).map_err(|_| StoreError)
    }

    fn delete(&self, id: &str) {
        if let Some(path) = self.path(id) {
            let _ = std::fs::remove_file(path);
        }
    }

    fn ids(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                let id = name.strip_suffix(".bin")?;
                (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| id.to_owned())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_go_on_delete() {
        let dir = std::env::temp_dir().join(format!("pwm-helper-{}", std::process::id()));
        let store = FileStore::at(dir.clone());
        let id = "0123456789abcdef0123456789abcdef";
        store.set(id, b"record-bytes").unwrap();
        let file = std::fs::read(dir.join(format!("{id}.bin"))).unwrap();
        // DPAPI on Windows; on Linux, a file only this user can read.
        #[cfg(windows)]
        assert!(!file.windows(12).any(|w| w == b"record-bytes"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(format!("{id}.bin")))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            assert_eq!(file, b"record-bytes");
        }
        assert_eq!(store.get(id).unwrap(), b"record-bytes");
        assert_eq!(store.ids(), [id]);
        let other = "fedcba9876543210fedcba9876543210";
        std::fs::copy(
            dir.join(format!("{id}.bin")),
            dir.join(format!("{other}.bin")),
        )
        .unwrap();
        // On Windows, another enrolment's file under this id doesn't open.
        #[cfg(windows)]
        assert!(store.get(other).is_none());
        store.delete(id);
        assert!(store.get(id).is_none());
        assert!(store.get("..\\x").is_none());
        // The folder stays while an enrolment is left, then goes.
        store.remove_if_empty();
        assert!(dir.exists());
        store.delete(other);
        store.remove_if_empty();
        assert!(!dir.exists());
    }
}
