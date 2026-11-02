//! Item model, vault/account structs and versioned formats.
#![forbid(unsafe_code)]

mod codec;
mod error;
mod history;
mod id;
mod item;
mod string_enum;
mod totp;

pub use codec::{MAX_SEALED_ITEM_BYTES, decrypt_item, encrypt_item, encrypt_item_fitting};
pub use error::VaultError;
pub use history::{HISTORY_LIMIT, ItemVersion, TRASH_RETENTION_MS, trash_folder};
pub use id::{ItemId, ParseIdError};
pub use item::{
    Card, CustomField, FieldKind, Folder, Identity, Item, ItemKind, Login, LoginUrl, Passkey,
    SCHEMA_VERSION, UrlMatch, collect_tags, remove_folder_refs,
};
pub use totp::{Totp, TotpCode, TotpKind, normalize_totp};
