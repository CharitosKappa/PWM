//! Item model, schema v1 (decision D18).
//!
//! Items are JSON. Fields this version does not know are kept in `extra` maps
//! and written back unchanged, so an older client never drops data written by
//! a newer one. Items with a newer schema or an unknown type are read-only.

use std::collections::BTreeSet;
use std::fmt;

use pwm_crypto::Secret;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zeroize::Zeroize;

use crate::string_enum::string_enum;
use crate::{ItemId, VaultError};

/// Item schema version written by this build.
pub const SCHEMA_VERSION: u32 = 1;

/// Unknown JSON fields, kept for forward compatibility.
type Extra = Map<String, Value>;

string_enum!(
    /// How a login URL matches a page (see `docs/AUTOFILL.md`).
    #[derive(Default)]
    UrlMatch {
        /// Registrable domain (Public Suffix List). The default.
        #[default]
        Domain => "domain",
        Host => "host",
        StartsWith => "starts_with",
        Regex => "regex",
        Exact => "exact",
        Never => "never",
    }
);

string_enum!(
    /// How a custom field is shown and edited.
    #[derive(Default)]
    FieldKind {
        #[default]
        Text => "text",
        /// Masked like a password.
        Hidden => "hidden",
        Url => "url",
        Date => "date",
    }
);

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoginUrl {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub r#match: UrlMatch,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Passkey {
    #[serde(default, with = "b64")]
    pub credential_id: Vec<u8>,
    #[serde(default)]
    pub rp_id: String,
    #[serde(default, with = "b64")]
    pub user_handle: Vec<u8>,
    #[serde(default)]
    pub user_name: String,
    /// PKCS#8 private key.
    #[serde(default, with = "b64")]
    pub private_key: Vec<u8>,
    #[serde(default)]
    pub sign_count: u32,
    #[serde(default)]
    pub created_at: i64,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Login {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub urls: Vec<LoginUrl>,
    /// `otpauth://` URI.
    #[serde(default)]
    pub totp: Option<String>,
    #[serde(default)]
    pub passkeys: Vec<Passkey>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Card {
    #[serde(default)]
    pub cardholder: String,
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub expiry_month: Option<u8>,
    #[serde(default)]
    pub expiry_year: Option<u16>,
    #[serde(default)]
    pub security_code: String,
    #[serde(default)]
    pub brand: String,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Identity {
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub middle_name: String,
    #[serde(default)]
    pub last_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub address_lines: Vec<String>,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub postal_code: String,
    #[serde(default)]
    pub country: String,
    #[serde(flatten)]
    extra: Extra,
}

/// A folder is stored like an item; its name is the item title.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    #[serde(default)]
    pub parent_id: Option<ItemId>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomField {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub kind: FieldKind,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Clone, PartialEq)]
pub enum ItemKind {
    Login(Login),
    Note,
    Card(Card),
    Identity(Identity),
    Folder(Folder),
    /// A type from a newer app version; its data is kept but not editable.
    Unknown(String),
}

impl ItemKind {
    fn type_name(&self) -> &str {
        match self {
            Self::Login(_) => "login",
            Self::Note => "note",
            Self::Card(_) => "card",
            Self::Identity(_) => "identity",
            Self::Folder(_) => "folder",
            Self::Unknown(name) => name,
        }
    }
}

/// A decrypted vault item. Zeroized on drop; `Debug` is redacted.
#[derive(Clone, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub title: String,
    pub notes: String,
    pub favorite: bool,
    pub folder_id: Option<ItemId>,
    pub tags: Vec<String>,
    pub custom_fields: Vec<CustomField>,
    /// Unix time in milliseconds.
    pub created_at: i64,
    /// Unix time in milliseconds.
    pub updated_at: i64,
    schema_version: u32,
    extra: Extra,
}

impl Item {
    /// A new item with the current schema version.
    pub fn new(kind: ItemKind, title: impl Into<String>, now_ms: i64) -> Self {
        Self {
            kind,
            title: title.into(),
            notes: String::new(),
            favorite: false,
            folder_id: None,
            tags: Vec::new(),
            custom_fields: Vec::new(),
            created_at: now_ms,
            updated_at: now_ms,
            schema_version: SCHEMA_VERSION,
            extra: Extra::new(),
        }
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// False for items written by a newer schema or of an unknown type: they
    /// are shown read-only so this version cannot corrupt them.
    pub fn is_editable(&self) -> bool {
        self.schema_version <= SCHEMA_VERSION && !matches!(self.kind, ItemKind::Unknown(_))
    }

    /// Parses item JSON. Errors never include the input.
    pub fn from_json(json: &str) -> Result<Self, VaultError> {
        Self::from_slice(json.as_bytes())
    }

    pub(crate) fn from_slice(json: &[u8]) -> Result<Self, VaultError> {
        let raw: RawItem = serde_json::from_slice(json).map_err(|_| VaultError::InvalidItem)?;
        Self::try_from(upgrade(raw)?)
    }

    /// Serializes the item to JSON.
    pub fn to_json(&self) -> Result<Secret<String>, VaultError> {
        let mut failed = false;
        let json =
            Secret::<String>::new_with(|s| match serde_json::to_string(&RawItemOut::from(self)) {
                Ok(mut out) => {
                    s.push_str(&out);
                    out.zeroize();
                }
                Err(_) => failed = true,
            });
        if failed {
            Err(VaultError::InvalidItem)
        } else {
            Ok(json)
        }
    }

    pub(crate) fn write_json(&self, buf: &mut Vec<u8>) -> Result<(), VaultError> {
        serde_json::to_writer(buf, &RawItemOut::from(self)).map_err(|_| VaultError::InvalidItem)
    }

    /// Checks values that would break autofill or display.
    pub fn validate(&self) -> Result<(), VaultError> {
        match &self.kind {
            ItemKind::Login(login) if login.urls.iter().any(|u| u.url.trim().is_empty()) => {
                Err(VaultError::Invalid("login URL is empty"))
            }
            ItemKind::Card(card) if card.expiry_month.is_some_and(|m| !(1..=12).contains(&m)) => {
                Err(VaultError::Invalid("card expiry month must be 1-12"))
            }
            _ => Ok(()),
        }
    }
}

impl fmt::Debug for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Item([REDACTED])")
    }
}

macro_rules! redacted_debug {
    ($($name:ident),+) => {
        $(impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "([REDACTED])"))
            }
        })+
    };
}

redacted_debug!(
    LoginUrl,
    Passkey,
    Login,
    Card,
    Identity,
    Folder,
    CustomField,
    ItemKind
);

/// All tags used by `items`, sorted and without duplicates.
pub fn collect_tags(items: &[Item]) -> Vec<String> {
    let tags: BTreeSet<&String> = items.iter().flat_map(|i| &i.tags).collect();
    tags.into_iter().cloned().collect()
}

/// Moves every item in `folder` to no folder (after the folder is deleted).
/// Returns how many items changed.
pub fn remove_folder_refs(items: &mut [Item], folder: &ItemId) -> usize {
    let mut changed = 0;
    for item in items
        .iter_mut()
        .filter(|i| i.folder_id.as_ref() == Some(folder))
    {
        item.folder_id = None;
        changed += 1;
    }
    changed
}

// --- JSON container ---

#[derive(Deserialize)]
struct RawItem {
    v: u32,
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    favorite: bool,
    #[serde(default)]
    folder_id: Option<ItemId>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    custom_fields: Vec<CustomField>,
    created_at: i64,
    updated_at: i64,
    #[serde(default)]
    login: Option<Login>,
    #[serde(default)]
    card: Option<Card>,
    #[serde(default)]
    identity: Option<Identity>,
    #[serde(default)]
    folder: Option<Folder>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(Serialize)]
struct RawItemOut<'a> {
    v: u32,
    #[serde(rename = "type")]
    item_type: &'a str,
    title: &'a str,
    notes: &'a str,
    favorite: bool,
    folder_id: Option<ItemId>,
    tags: &'a [String],
    custom_fields: &'a [CustomField],
    created_at: i64,
    updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    login: Option<&'a Login>,
    #[serde(skip_serializing_if = "Option::is_none")]
    card: Option<&'a Card>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<&'a Identity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    folder: Option<&'a Folder>,
    #[serde(flatten)]
    extra: &'a Extra,
}

impl<'a> From<&'a Item> for RawItemOut<'a> {
    fn from(item: &'a Item) -> Self {
        let (mut login, mut card, mut identity, mut folder) = (None, None, None, None);
        match &item.kind {
            ItemKind::Login(l) => login = Some(l),
            ItemKind::Card(c) => card = Some(c),
            ItemKind::Identity(i) => identity = Some(i),
            ItemKind::Folder(f) => folder = Some(f),
            ItemKind::Note | ItemKind::Unknown(_) => {}
        }
        Self {
            v: item.schema_version,
            item_type: item.kind.type_name(),
            title: &item.title,
            notes: &item.notes,
            favorite: item.favorite,
            folder_id: item.folder_id,
            tags: &item.tags,
            custom_fields: &item.custom_fields,
            created_at: item.created_at,
            updated_at: item.updated_at,
            login,
            card,
            identity,
            folder,
            extra: &item.extra,
        }
    }
}

/// Brings an older schema up to [`SCHEMA_VERSION`]. Newer schemas pass
/// through unchanged and are read-only. When the schema changes, add the
/// migration here plus a test that `tests/fixtures/items_v1.json` still loads.
fn upgrade(raw: RawItem) -> Result<RawItem, VaultError> {
    match raw.v {
        0 => Err(VaultError::InvalidItem),
        _ => Ok(raw),
    }
}

impl TryFrom<RawItem> for Item {
    type Error = VaultError;

    fn try_from(mut raw: RawItem) -> Result<Self, VaultError> {
        let payloads = [
            raw.login.is_some(),
            raw.card.is_some(),
            raw.identity.is_some(),
            raw.folder.is_some(),
        ]
        .iter()
        .filter(|&&p| p)
        .count();
        let kind = match raw.item_type.as_str() {
            "login" => raw.login.take().map(ItemKind::Login),
            "card" => raw.card.take().map(ItemKind::Card),
            "identity" => raw.identity.take().map(ItemKind::Identity),
            "folder" => raw.folder.take().map(ItemKind::Folder),
            "note" if payloads == 0 => Some(ItemKind::Note),
            "note" => None,
            other if payloads == 0 => Some(ItemKind::Unknown(other.to_string())),
            _ => None,
        };
        // Exactly the payload that matches the type, nothing else.
        let kind = match kind {
            Some(kind) if payloads <= 1 => kind,
            _ => return Err(VaultError::InvalidItem),
        };
        Ok(Self {
            kind,
            title: std::mem::take(&mut raw.title),
            notes: std::mem::take(&mut raw.notes),
            favorite: raw.favorite,
            folder_id: raw.folder_id,
            tags: std::mem::take(&mut raw.tags),
            custom_fields: std::mem::take(&mut raw.custom_fields),
            created_at: raw.created_at,
            updated_at: raw.updated_at,
            schema_version: raw.v,
            extra: std::mem::take(&mut raw.extra),
        })
    }
}

// --- Zeroize on drop ---

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(values) => values.iter_mut().for_each(zeroize_value),
        Value::Object(map) => zeroize_map(map),
        _ => {}
    }
}

fn zeroize_map(map: &mut Extra) {
    map.values_mut().for_each(zeroize_value);
    map.clear();
}

impl Zeroize for LoginUrl {
    fn zeroize(&mut self) {
        self.url.zeroize();
        self.r#match.zeroize();
        zeroize_map(&mut self.extra);
    }
}

impl Zeroize for Passkey {
    fn zeroize(&mut self) {
        self.credential_id.zeroize();
        self.rp_id.zeroize();
        self.user_handle.zeroize();
        self.user_name.zeroize();
        self.private_key.zeroize();
        zeroize_map(&mut self.extra);
    }
}

impl Zeroize for Login {
    fn zeroize(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
        self.urls.zeroize();
        self.totp.zeroize();
        self.passkeys.zeroize();
        zeroize_map(&mut self.extra);
    }
}

impl Zeroize for Card {
    fn zeroize(&mut self) {
        self.cardholder.zeroize();
        self.number.zeroize();
        self.security_code.zeroize();
        self.brand.zeroize();
        self.expiry_month.zeroize();
        self.expiry_year.zeroize();
        zeroize_map(&mut self.extra);
    }
}

impl Zeroize for Identity {
    fn zeroize(&mut self) {
        for field in [
            &mut self.first_name,
            &mut self.middle_name,
            &mut self.last_name,
            &mut self.email,
            &mut self.phone,
            &mut self.company,
            &mut self.city,
            &mut self.region,
            &mut self.postal_code,
            &mut self.country,
        ] {
            field.zeroize();
        }
        self.address_lines.zeroize();
        zeroize_map(&mut self.extra);
    }
}

impl Zeroize for Folder {
    fn zeroize(&mut self) {
        zeroize_map(&mut self.extra);
    }
}

impl Zeroize for CustomField {
    fn zeroize(&mut self) {
        self.name.zeroize();
        self.value.zeroize();
        self.kind.zeroize();
        zeroize_map(&mut self.extra);
    }
}

impl Drop for Item {
    fn drop(&mut self) {
        match &mut self.kind {
            ItemKind::Login(l) => l.zeroize(),
            ItemKind::Card(c) => c.zeroize(),
            ItemKind::Identity(i) => i.zeroize(),
            ItemKind::Folder(f) => f.zeroize(),
            ItemKind::Unknown(name) => name.zeroize(),
            ItemKind::Note => {}
        }
        self.title.zeroize();
        self.notes.zeroize();
        self.tags.zeroize();
        self.custom_fields.zeroize();
        zeroize_map(&mut self.extra);
    }
}

impl Drop for RawItem {
    fn drop(&mut self) {
        self.title.zeroize();
        self.notes.zeroize();
        self.tags.zeroize();
        self.custom_fields.zeroize();
        self.login.zeroize();
        self.card.zeroize();
        self.identity.zeroize();
        self.folder.zeroize();
        zeroize_map(&mut self.extra);
    }
}

/// Base64url without padding for binary passkey fields.
mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde::{Deserialize, Deserializer, Serializer};
    use zeroize::Zeroize;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = URL_SAFE_NO_PAD.encode(bytes);
        let result = serializer.serialize_str(&text);
        text.zeroize();
        result
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let mut text = String::deserialize(deserializer)?;
        let result = URL_SAFE_NO_PAD
            .decode(&text)
            .map_err(|_| serde::de::Error::custom("invalid base64url"));
        text.zeroize();
        result
    }
}
