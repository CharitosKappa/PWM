//! Encrypted (`.pwmx`) and plaintext (JSON, CSV) exports of our own vaults,
//! and the readers for them (decision D24). No file I/O: the platform saves
//! and opens the bytes through its own file picker.
#![forbid(unsafe_code)]

mod csv;
mod error;
mod grant;
mod json;
mod pwmx;

use pwm_crypto::Id;
use pwm_vault::{Item, ItemId};

pub use csv::{CSV_COLUMNS, export_csv, read_csv};
pub use error::ExportError;
pub use grant::{ExportGrant, GRANT_LIFETIME_MS, authorize_export};
pub use json::{export_json, read_json};
pub use pwmx::{check_export_password, export_pwmx, read_pwmx};

/// An item with its id.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportItem {
    pub id: ItemId,
    pub item: Item,
}

/// One vault's items, folders included.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportVault {
    pub id: Id,
    pub items: Vec<ExportItem>,
}

/// A parsed JSON or `.pwmx` export.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    /// Unix time in milliseconds.
    pub exported_at: i64,
    pub vaults: Vec<ExportVault>,
}
