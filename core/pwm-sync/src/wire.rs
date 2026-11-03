//! The D27 JSON shapes the engine reads and writes.

use std::str::FromStr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::{Envelope, Id};
use pwm_vault::ItemId;
use serde::{Deserialize, Serialize};

pub(crate) fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn unb64(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).ok()
}

/// A revision or seq as the server sends it: canonical decimal (D27).
pub(crate) fn counter(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical { text.parse().ok() } else { None }
}

pub(crate) fn id_text(id: &Id) -> String {
    ItemId::from_bytes(*id).to_string()
}

pub(crate) fn parse_id(text: &str) -> Option<ItemId> {
    ItemId::from_str(text).ok()
}

fn envelope(text: &str) -> Option<Envelope> {
    Envelope::from_bytes(&unb64(text)?).ok()
}

#[derive(Deserialize)]
struct VaultJson {
    id: String,
    wrap: String,
    wrapped_key: String,
}

/// A vault the caller belongs to, with its key sealed under `sym_key`.
pub(crate) struct OwnVault {
    pub id: Id,
    pub wrapped_key: Envelope,
}

/// `GET /v1/vaults`: the vaults listed, and those wrapped for the caller's
/// own key. Shared vaults (`wrap: share`) arrive with the sharing milestone.
pub(crate) fn vault_list(body: &[u8]) -> Option<(Vec<Id>, Vec<OwnVault>)> {
    let list: Vec<VaultJson> = serde_json::from_slice(body).ok()?;
    let mut listed = Vec::new();
    let mut own = Vec::new();
    for v in list {
        let id = *parse_id(&v.id)?.as_bytes();
        listed.push(id);
        if v.wrap == "own" {
            own.push(OwnVault {
                id,
                wrapped_key: envelope(&v.wrapped_key)?,
            });
        }
    }
    Some((listed, own))
}

#[derive(Deserialize)]
struct ChangeJson {
    id: String,
    revision: String,
    seq: String,
    #[serde(default)]
    blob: Option<String>,
    #[serde(default)]
    deleted: bool,
}

#[derive(Deserialize)]
struct ChangesJson {
    changes: Vec<ChangeJson>,
    next: String,
    more: bool,
}

/// One change from delta sync: `envelope` is `None` for a tombstone.
pub(crate) struct Change {
    pub id: ItemId,
    pub revision: u64,
    pub envelope: Option<Envelope>,
}

pub(crate) struct Page {
    pub changes: Vec<Change>,
    pub next: u64,
    pub more: bool,
}

pub(crate) fn page(body: &[u8]) -> Option<Page> {
    let page: ChangesJson = serde_json::from_slice(body).ok()?;
    let changes = page
        .changes
        .into_iter()
        .map(|c| {
            counter(&c.seq)?;
            let envelope = match (c.deleted, c.blob) {
                (true, None) => None,
                (false, Some(blob)) => Some(envelope(&blob)?),
                _ => return None,
            };
            Some(Change {
                id: parse_id(&c.id)?,
                revision: counter(&c.revision)?,
                envelope,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Page {
        changes,
        next: counter(&page.next)?,
        more: page.more,
    })
}

#[derive(Deserialize)]
struct WrittenJson {
    seq: String,
}

/// The `seq` of a successful write or delete.
pub(crate) fn written_seq(body: &[u8]) -> Option<u64> {
    counter(&serde_json::from_slice::<WrittenJson>(body).ok()?.seq)
}

#[derive(Deserialize)]
struct CurrentJson {
    revision: String,
    blob: String,
}

#[derive(Deserialize)]
struct ConflictJson {
    current: CurrentJson,
}

/// The current item sent with a 412.
pub(crate) fn conflict(body: &[u8]) -> Option<(u64, Envelope)> {
    let c = serde_json::from_slice::<ConflictJson>(body).ok()?.current;
    Some((counter(&c.revision)?, envelope(&c.blob)?))
}

#[derive(Deserialize)]
struct ErrorJson {
    error: String,
}

pub(crate) fn error_code(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<ErrorJson>(body)
        .ok()
        .map(|e| e.error)
}

#[derive(Deserialize)]
pub(crate) struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Serialize)]
pub(crate) struct ItemWrite {
    pub revision: String,
    pub blob: String,
}

#[derive(Serialize)]
pub(crate) struct NewVault {
    pub id: String,
    pub wrapped_key: String,
}

#[derive(Serialize)]
pub(crate) struct Refresh<'a> {
    pub refresh_token: &'a str,
}
