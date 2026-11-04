//! Importers for other password managers' exports, and our own (decision
//! D51). No I/O: the platform reads the picked file into memory and passes
//! the bytes. Each format becomes D18 items with new ids, folders first, and
//! a report of what came in and what was left out. Nothing is logged, and
//! errors never quote the file.
//!
//! Formats are implemented from each vendor's published format notes and
//! sample exports we wrote ourselves; no code comes from other managers.
#![forbid(unsafe_code)]

mod bitwarden;
mod bitwarden_crypto;
mod build;
mod csv;
mod dashlane;
mod error;
mod generic;
mod json;
mod keepass;
mod keeper;
mod lastpass;
mod nordpass;
mod onepassword;
mod own;
mod proton;
mod zipfile;

use pwm_crypto::Secret;
use pwm_vault::{Item, ItemId};

pub use build::{LeftOut, Report};
pub use error::ImportError;
pub use generic::{ColumnMapping, Columns, csv_columns};
pub use keepass::{
    MAX_AES_ROUNDS, MAX_ARGON2_ITERATIONS, MAX_ARGON2_MEMORY, MAX_ARGON2_PARALLELISM,
};

/// Files larger than this are refused (D51 §7).
pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
/// At most this many items (folders included) per import.
pub const MAX_ITEMS: usize = 50_000;
/// A ZIP export: at most this many entries...
pub const MAX_ZIP_ENTRIES: usize = 10_000;
/// ...and this much unpacked data in the entries read.
pub const MAX_UNPACKED_BYTES: u64 = 200 * 1024 * 1024;

/// The formats `parse` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Bitwarden JSON, unencrypted or password-protected.
    Bitwarden,
    /// 1Password `.1pux`.
    OnePassword,
    /// Keeper JSON.
    Keeper,
    /// KeePass `.kdbx`, versions 3.1 and 4 (D51 §5).
    KeePass,
    /// NordPass CSV.
    NordPass,
    /// Proton Pass ZIP (unencrypted) or its `data.json`.
    ProtonPass,
    /// Dashlane CSV ZIP, a lone `credentials.csv`, or its JSON.
    Dashlane,
    /// LastPass CSV.
    LastPass,
    /// Any CSV with a header row, through a [`ColumnMapping`]; browser
    /// exports are mapped by their headers.
    Csv,
    /// Our own `.pwmx`, JSON or CSV export (D24).
    Pwm,
}

impl Format {
    pub const ALL: [Format; 10] = [
        Format::Bitwarden,
        Format::OnePassword,
        Format::Keeper,
        Format::KeePass,
        Format::NordPass,
        Format::ProtonPass,
        Format::Dashlane,
        Format::LastPass,
        Format::Csv,
        Format::Pwm,
    ];

    /// A stable name for the bindings.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bitwarden => "bitwarden",
            Self::OnePassword => "1password",
            Self::Keeper => "keeper",
            Self::KeePass => "keepass",
            Self::NordPass => "nordpass",
            Self::ProtonPass => "protonpass",
            Self::Dashlane => "dashlane",
            Self::LastPass => "lastpass",
            Self::Csv => "csv",
            Self::Pwm => "pwm",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == name)
    }
}

/// What [`detect`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    pub format: Format,
    /// The file is encrypted: `parse` needs its password.
    pub needs_password: bool,
}

/// What `parse` needs besides the bytes.
#[derive(Default)]
pub struct Options {
    /// The export's own password (Bitwarden's password-protected export,
    /// our `.pwmx`).
    pub password: Option<Secret<String>>,
    /// A KeePass key file, with or instead of the password.
    pub key_file: Option<Secret<Vec<u8>>>,
    /// Which column holds what, for [`Format::Csv`]; `None` takes the
    /// suggestion from [`csv_columns`].
    pub mapping: Option<ColumnMapping>,
    /// Unix ms, for items whose source has no times.
    pub now_ms: i64,
}

/// The import, ready to save: folders first, then items, all with new ids.
/// Folder references point at folders in the same list.
pub struct Parsed {
    pub items: Vec<(ItemId, Item)>,
    pub report: Report,
}

impl std::fmt::Debug for Parsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parsed")
            .field("items", &self.items.len())
            .field("report", &self.report)
            .finish()
    }
}

/// Suggests a format from the content; the file name is only a hint (a
/// `.csv` that is not one of the known exports is a generic CSV). `None`:
/// not a file we read.
pub fn detect(file_name: &str, bytes: &[u8]) -> Option<Detected> {
    if bytes.len() > MAX_FILE_BYTES {
        return None;
    }
    let plain = |format| {
        Some(Detected {
            format,
            needs_password: false,
        })
    };
    if keepass::is_kdbx(bytes) {
        return Some(Detected {
            format: Format::KeePass,
            needs_password: true,
        });
    }
    if own::is_pwmx(bytes) {
        return Some(Detected {
            format: Format::Pwm,
            needs_password: true,
        });
    }
    if zipfile::is_zip(bytes) {
        return zipfile::detect(bytes).map(|format| Detected {
            format,
            needs_password: false,
        });
    }
    let text = csv::strip_bom(bytes);
    let first = text.iter().copied().find(|b| !b.is_ascii_whitespace());
    if first == Some(b'{') {
        let value = json::parse(text).ok()?;
        return json_format(&value);
    }
    let name = file_name.to_ascii_lowercase();
    let headers = csv::headers(text)?;
    let headers: Vec<String> = headers
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    if own::is_own_csv(&headers) {
        return plain(Format::Pwm);
    }
    if lastpass::is_lastpass(&headers) {
        return plain(Format::LastPass);
    }
    if nordpass::is_nordpass(&headers) {
        return plain(Format::NordPass);
    }
    if dashlane::is_credentials_csv(&headers) {
        return plain(Format::Dashlane);
    }
    if [".json", ".pwmx", ".1pux", ".kdbx", ".kdb"]
        .iter()
        .any(|e| name.ends_with(e))
    {
        return None;
    }
    plain(Format::Csv)
}

fn json_format(value: &json::Doc) -> Option<Detected> {
    let v = value.get();
    let plain = |format| {
        Some(Detected {
            format,
            needs_password: false,
        })
    };
    if v.get("format").and_then(|f| f.as_str()) == Some("pwm-export") {
        return plain(Format::Pwm);
    }
    if v.get("encrypted").is_some() && (v.get("items").is_some() || v.get("data").is_some()) {
        let needs_password = v.get("passwordProtected").and_then(|p| p.as_bool()) == Some(true);
        return Some(Detected {
            format: Format::Bitwarden,
            needs_password,
        });
    }
    if v.get("records").is_some() {
        return plain(Format::Keeper);
    }
    if v.get("vaults").is_some() && v.get("userId").is_some() {
        return plain(Format::ProtonPass);
    }
    if v.get("AUTHENTIFIANT").is_some() || v.get("SECURENOTE").is_some() {
        return plain(Format::Dashlane);
    }
    None
}

/// Reads `bytes` as `format`. The bytes are the caller's; parsed values are
/// zeroized as the parse goes, items when dropped.
pub fn parse(format: Format, bytes: &[u8], options: &Options) -> Result<Parsed, ImportError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ImportError::TooLarge);
    }
    let now = options.now_ms;
    match format {
        Format::Bitwarden => bitwarden::parse(bytes, options.password.as_ref(), now),
        Format::OnePassword => onepassword::parse(bytes, now),
        Format::Keeper => keeper::parse(bytes, now),
        Format::KeePass => keepass::parse(
            bytes,
            options.password.as_ref(),
            options.key_file.as_ref(),
            now,
        ),
        Format::NordPass => nordpass::parse(bytes, now),
        Format::ProtonPass => proton::parse(bytes, now),
        Format::Dashlane => dashlane::parse(bytes, now),
        Format::LastPass => lastpass::parse(bytes, now),
        Format::Csv => generic::parse(bytes, options.mapping.as_ref(), now),
        Format::Pwm => own::parse(bytes, options.password.as_ref(), now),
    }
}
