//! ZIP exports (1Password `.1pux`, Proton Pass, Dashlane): only the entries
//! we expect are read, within the entry and size limits (D51 §7), so a ZIP
//! bomb stops at the limit instead of filling memory.

use std::io::{Cursor, Read};

use zeroize::Zeroizing;
use zip::ZipArchive;

use crate::{Format, ImportError, MAX_UNPACKED_BYTES, MAX_ZIP_ENTRIES};

pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

pub struct Zip<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    unpacked: u64,
    limit: u64,
}

impl<'a> Zip<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, ImportError> {
        Self::open_with_limit(bytes, MAX_UNPACKED_BYTES)
    }

    fn open_with_limit(bytes: &'a [u8], limit: u64) -> Result<Self, ImportError> {
        let archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| ImportError::Unreadable)?;
        if archive.len() > MAX_ZIP_ENTRIES {
            return Err(ImportError::TooLarge);
        }
        Ok(Self {
            archive,
            unpacked: 0,
            limit,
        })
    }

    /// Entry names, as stored.
    pub fn names(&self) -> Vec<String> {
        self.archive
            .file_names()
            .filter_map(Result::ok)
            .map(|n| n.into_owned())
            .collect()
    }

    /// The first entry whose name, without folders, is `file` (case
    /// insensitive).
    pub fn find(&self, file: &str) -> Option<String> {
        self.archive
            .file_names()
            .filter_map(Result::ok)
            .find(|n| {
                n.rsplit('/')
                    .next()
                    .is_some_and(|base| base.eq_ignore_ascii_case(file))
            })
            .map(|n| n.into_owned())
    }

    /// Reads one entry, counting what it unpacks to against the limit.
    pub fn read(&mut self, name: &str) -> Result<Zeroizing<Vec<u8>>, ImportError> {
        let entry = self
            .archive
            .by_name(name)
            .map_err(|_| ImportError::Unreadable)?;
        let left = self.limit.saturating_sub(self.unpacked);
        let mut out = Zeroizing::new(Vec::new());
        // One byte past the limit tells a full entry from a too-large one.
        entry
            .take(left + 1)
            .read_to_end(&mut out)
            .map_err(|_| ImportError::Unreadable)?;
        let read = out.len() as u64;
        if read > left {
            return Err(ImportError::TooLarge);
        }
        self.unpacked += read;
        Ok(out)
    }
}

/// Which export a ZIP is, from its entries.
pub fn detect(bytes: &[u8]) -> Option<Format> {
    let zip = Zip::open(bytes).ok()?;
    if zip.find("export.data").is_some() {
        return Some(Format::OnePassword);
    }
    if zip.find("credentials.csv").is_some() || zip.find("securenotes.csv").is_some() {
        return Some(Format::Dashlane);
    }
    if zip.find("data.json").is_some() {
        return Some(Format::ProtonPass);
    }
    None
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn zip_of(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in files {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn a_zip_bomb_stops_at_the_limit() {
        // 100 KB of zeros packs into a few hundred bytes.
        let bomb = zip_of(&[("export.data", vec![0u8; 100_000]), ("b", vec![0u8; 600])]);
        assert!(bomb.len() < 2_000);
        let mut zip = Zip::open_with_limit(&bomb, 1_000).unwrap();
        assert!(matches!(
            zip.read("export.data"),
            Err(ImportError::TooLarge)
        ));
        // What fits is read, and counts toward the total.
        let mut zip = Zip::open_with_limit(&bomb, 1_000).unwrap();
        assert_eq!(zip.read("b").unwrap().len(), 600);
        assert!(matches!(zip.read("b"), Err(ImportError::TooLarge)));
    }

    #[test]
    fn entries_are_found_by_their_file_name() {
        let zip = zip_of(&[("Proton Pass/data.json", b"{}".to_vec())]);
        let zip = Zip::open(&zip).unwrap();
        assert_eq!(
            zip.find("DATA.JSON").as_deref(),
            Some("Proton Pass/data.json")
        );
        assert_eq!(zip.find("other.json"), None);
    }
}
