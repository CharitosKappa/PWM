//! Each enrolment's record (the challenge and the sealed K, never K) in
//! `%LOCALAPPDATA%\PWM\browser-unlock\<id>.bin`, protected with DPAPI for
//! this Windows user, with the id as DPAPI's entropy (D69 §2).

use std::path::PathBuf;

use crate::{Store, StoreError};
use pwm_hello::dpapi;

pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new() -> Option<Self> {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(Self::at(
            PathBuf::from(base).join("PWM").join("browser-unlock"),
        ))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
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
        dpapi::unprotect(id, &sealed).ok()
    }

    fn set(&self, id: &str, record: &[u8]) -> Result<(), StoreError> {
        let path = self.path(id).ok_or(StoreError)?;
        let sealed = dpapi::protect(id, record).map_err(|_| StoreError)?;
        std::fs::create_dir_all(&self.dir).map_err(|_| StoreError)?;
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, sealed).map_err(|_| StoreError)?;
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
    fn records_round_trip_sealed_and_go_on_delete() {
        let dir = std::env::temp_dir().join(format!("pwm-helper-{}", std::process::id()));
        let store = FileStore::at(dir.clone());
        let id = "0123456789abcdef0123456789abcdef";
        store.set(id, b"record-bytes").unwrap();
        let file = std::fs::read(dir.join(format!("{id}.bin"))).unwrap();
        assert!(!file.windows(12).any(|w| w == b"record-bytes"));
        assert_eq!(store.get(id).unwrap(), b"record-bytes");
        assert_eq!(store.ids(), [id]);
        // Another enrolment's file under this id doesn't open.
        let other = "fedcba9876543210fedcba9876543210";
        std::fs::copy(
            dir.join(format!("{id}.bin")),
            dir.join(format!("{other}.bin")),
        )
        .unwrap();
        assert!(store.get(other).is_none());
        store.delete(id);
        assert!(store.get(id).is_none());
        assert!(store.get("..\\x").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
