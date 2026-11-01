//! Item model, vault/account structs and versioned formats.
#![forbid(unsafe_code)]

mod codec;
mod error;
mod id;
mod item;
mod string_enum;

pub use codec::{decrypt_item, encrypt_item};
pub use error::VaultError;
pub use id::{ItemId, ParseIdError};
pub use item::{
    Card, CustomField, FieldKind, Folder, Identity, Item, ItemKind, Login, LoginUrl, Passkey,
    SCHEMA_VERSION, UrlMatch, collect_tags, remove_folder_refs,
};
