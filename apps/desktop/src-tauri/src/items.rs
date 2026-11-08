//! The desktop's item screens (M7.7, D67): the vault's list, an item to
//! show with every hidden value left out, Show and Copy by a value's place,
//! the editor's copy of an item, saving, trash and folders.
//!
//! What reaches the page (D67 §1): lists and details never hold a hidden
//! value; `reveal` answers one value after Show; `item_for_edit` answers the
//! whole item, without its history or its passkeys, which never leave the
//! backend.

use pwm_client::{ClientError, Secret};
use serde_json::{Map, Value, json};

use crate::backend::{Backend, Clock, Failure};
use crate::clipboard::Clipboard;
use crate::http::Transport;
use crate::keys::KeyStore;
use crate::protocol::{
    FolderView, ItemView, PasskeyView, PastPassword, TotpNow, VaultRow, VaultView,
};

type Result<T> = std::result::Result<T, Failure>;

/// The schema this build writes (D18): a newer one is read-only.
const SCHEMA_VERSION: u64 = 1;
const KNOWN_TYPES: [&str; 5] = ["login", "note", "card", "identity", "folder"];

fn text(value: &Value) -> String {
    value.as_str().unwrap_or("").to_owned()
}

fn non_empty(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.is_empty())
}

/// The places of an item's hidden values (D43, D67 §1): a login's password
/// and one-time code secret, a card's number and security code, hidden
/// custom fields, and the passwords in its history. Only places that hold
/// something are listed.
pub fn hidden_paths(item: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for path in [
        "/login/password",
        "/login/totp",
        "/card/number",
        "/card/security_code",
    ] {
        if item.pointer(path).is_some_and(non_empty) {
            out.push(path.to_owned());
        }
    }
    if let Some(fields) = item["custom_fields"].as_array() {
        for (i, field) in fields.iter().enumerate() {
            if field["kind"] == "hidden" && non_empty(&field["value"]) {
                out.push(format!("/custom_fields/{i}/value"));
            }
        }
    }
    if let Some(history) = item["history"].as_array() {
        for (i, version) in history.iter().enumerate() {
            if version.pointer("/login/password").is_some_and(non_empty) {
                out.push(format!("/history/{i}/login/password"));
            }
        }
    }
    out
}

/// The item as the detail shows it: without its history, each hidden value
/// as "", and each passkey down to what names it.
pub fn masked(item: &Value) -> Value {
    let mut out = item.clone();
    if let Some(map) = out.as_object_mut() {
        map.remove("history");
    }
    for path in hidden_paths(&out) {
        if let Some(value) = out.pointer_mut(&path) {
            *value = Value::String(String::new());
        }
    }
    if let Some(passkeys) = out
        .pointer_mut("/login/passkeys")
        .and_then(Value::as_array_mut)
    {
        for passkey in passkeys.iter_mut() {
            if let Some(map) = passkey.as_object_mut() {
                map.retain(|k, _| matches!(k.as_str(), "rp_id" | "user_name" | "created_at"));
            }
        }
    }
    out
}

/// Whether this build may edit the item (D18): its schema and type are known.
pub fn editable(item: &Value) -> bool {
    item["v"].as_u64().is_some_and(|v| v <= SCHEMA_VERSION)
        && KNOWN_TYPES.contains(&item["type"].as_str().unwrap_or(""))
}

/// The list's line under the title: the username, a card's last 4 digits,
/// or a name.
fn subtitle(item: &Value) -> String {
    match item["type"].as_str().unwrap_or("") {
        "login" => text(&item["login"]["username"]),
        "card" => {
            let digits: String = text(&item["card"]["number"])
                .chars()
                .filter(char::is_ascii_digit)
                .collect();
            if digits.len() >= 4 {
                format!("•••• {}", &digits[digits.len() - 4..])
            } else {
                String::new()
            }
        }
        "identity" => {
            let first = text(&item["identity"]["first_name"]);
            let last = text(&item["identity"]["last_name"]);
            format!("{first} {last}").trim().to_owned()
        }
        _ => String::new(),
    }
}

fn parse(json: &Secret<String>) -> Result<Value> {
    serde_json::from_str(json.expose_secret()).map_err(|_| Failure::code("invalid_item"))
}

impl<T: Transport, K: KeyStore, C: Clipboard, Q: Clock> Backend<T, K, C, Q> {
    fn stored(&self, vault_id: &str, item_id: &str) -> Result<Value> {
        let json = self
            .client
            .item(vault_id, item_id)?
            .ok_or_else(|| Failure::code("unknown_item"))?;
        parse(&json)
    }

    /// The first vault, where new items go.
    fn first_vault(&self) -> Result<String> {
        self.client
            .vault_ids()?
            .into_iter()
            .next()
            .ok_or_else(|| Failure::code("no_vault"))
    }

    pub(crate) fn vault(&self) -> Result<VaultView> {
        let codes_included = self.client.codes_included(self.clock.now_ms())?;
        let mut items = Vec::new();
        let mut folders = Vec::new();
        for (vault_id, item) in self.stored_items()? {
            let item_id = text(&item["id"]);
            if item["type"] == "folder" {
                if item["trashed_at"].is_null() {
                    folders.push(FolderView {
                        vault_id,
                        item_id,
                        title: text(&item["title"]),
                    });
                }
                continue;
            }
            items.push(VaultRow {
                subtitle: subtitle(&item),
                url: text(&item["login"]["urls"][0]["url"]),
                r#type: text(&item["type"]),
                title: text(&item["title"]),
                favorite: item["favorite"].as_bool().unwrap_or(false),
                folder_id: item["folder_id"].as_str().map(str::to_owned),
                trashed: !item["trashed_at"].is_null(),
                has_password: item.pointer("/login/password").is_some_and(non_empty),
                has_code: codes_included && item.pointer("/login/totp").is_some_and(non_empty),
                vault_id,
                item_id,
            });
        }
        folders.sort_by_key(|f| f.title.to_lowercase());
        Ok(VaultView {
            items,
            folders,
            codes_included,
        })
    }

    pub(crate) fn item(&self, vault_id: &str, item_id: &str) -> Result<ItemView> {
        let item = self.stored(vault_id, item_id)?;
        // Without a plan that has codes, the screen says so (D73 §1); a code
        // the core can't read shows no code.
        let now = self
            .client
            .current_code(vault_id, item_id, self.clock.now_ms());
        let code_needs_plan = matches!(now, Err(ClientError::PlanRequired));
        let code = now.ok().flatten().map(|(code, seconds_left)| TotpNow {
            code: code.expose_secret().clone(),
            seconds_left,
        });
        let passkeys = item["login"]["passkeys"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|p| PasskeyView {
                        rp_id: text(&p["rp_id"]),
                        user_name: text(&p["user_name"]),
                        created_at: p["created_at"].as_i64().unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let password_history = item["history"]
            .as_array()
            .map(|history| {
                history
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.pointer("/login/password").is_some_and(non_empty))
                    .map(|(i, v)| PastPassword {
                        saved_at: v["saved_at"].as_i64().unwrap_or(0),
                        path: format!("/history/{i}/login/password"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(ItemView {
            vault_id: vault_id.to_owned(),
            item_id: item_id.to_owned(),
            hidden: hidden_paths(&item),
            read_only: !editable(&item),
            item: masked(&item),
            code,
            code_needs_plan,
            passkeys,
            password_history,
        })
    }

    /// One hidden value, after Show: only a place `hidden_paths` lists.
    pub(crate) fn reveal(&self, vault_id: &str, item_id: &str, path: &str) -> Result<String> {
        let item = self.stored(vault_id, item_id)?;
        if !hidden_paths(&item).iter().any(|p| p == path) {
            return Err(Failure::code("invalid_request"));
        }
        Ok(item.pointer(path).map(text).unwrap_or_default())
    }

    /// Copies any field by its place (D67 §1), with the 30-second clear.
    pub(crate) fn copy_path(&mut self, vault_id: &str, item_id: &str, path: &str) -> Result<()> {
        let item = self.stored(vault_id, item_id)?;
        let value = Secret::new(
            item.pointer(path)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        );
        self.write_clipboard(value.expose_secret())
    }

    /// The whole item for the editor, without its history and passkeys.
    pub(crate) fn item_for_edit(&self, vault_id: &str, item_id: &str) -> Result<Value> {
        let mut item = self.stored(vault_id, item_id)?;
        if !editable(&item) {
            return Err(Failure::code("read_only_item"));
        }
        if let Some(map) = item.as_object_mut() {
            map.remove("history");
        }
        if let Some(login) = item.get_mut("login").and_then(Value::as_object_mut) {
            login.remove("passkeys");
        }
        Ok(item)
    }

    /// Saves a new item (no `id`) or an edit. The page can't add, change or
    /// drop passkeys: the stored ones are always kept. The core checks the
    /// item and keeps the previous version in its history (D19).
    pub(crate) fn save_item(
        &mut self,
        vault_id: Option<&str>,
        item: Value,
    ) -> Result<(String, String)> {
        let Value::Object(mut item) = item else {
            return Err(Failure::code("invalid_item"));
        };
        let vault_id = match vault_id {
            Some(id) => id.to_owned(),
            None => self.first_vault()?,
        };
        let now = self.clock.now_ms();
        item.remove("history");
        let stored_passkeys = match item.get("id").and_then(Value::as_str) {
            Some(id) => self
                .client
                .item(&vault_id, id)?
                .map(|json| parse(&json))
                .transpose()?
                .and_then(|stored| stored.pointer("/login/passkeys").cloned()),
            None => {
                item.insert("id".to_owned(), json!(pwm_client::new_id()?));
                item.insert("v".to_owned(), json!(SCHEMA_VERSION));
                item.insert("created_at".to_owned(), json!(now));
                item.insert("updated_at".to_owned(), json!(now));
                None
            }
        };
        if let Some(login) = item.get_mut("login").and_then(Value::as_object_mut) {
            match stored_passkeys {
                Some(passkeys) => login.insert("passkeys".to_owned(), passkeys),
                None => login.remove("passkeys"),
            };
        }
        let json = Secret::new(Value::Object(item).to_string());
        let item_id = self
            .client
            .save_item(&vault_id, json.expose_secret(), now)?;
        Ok((vault_id, item_id))
    }

    pub(crate) fn set_trashed(
        &mut self,
        vault_id: &str,
        item_id: &str,
        trashed: bool,
    ) -> Result<()> {
        Ok(self
            .client
            .set_trashed(vault_id, item_id, trashed, self.clock.now_ms())?)
    }

    /// Deletes an item for good; the screen asks first.
    pub(crate) fn delete_forever(&mut self, vault_id: &str, item_id: &str) -> Result<()> {
        Ok(self.client.delete_item(vault_id, item_id)?)
    }

    pub(crate) fn new_folder(&mut self, title: &str) -> Result<(String, String)> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Failure::code("invalid_item"));
        }
        let mut folder = Map::new();
        folder.insert("type".to_owned(), json!("folder"));
        folder.insert("title".to_owned(), json!(title));
        folder.insert("folder".to_owned(), json!({ "parent_id": null }));
        self.save_item(None, Value::Object(folder))
    }

    /// A pasted `otpauth://` URI or bare secret, read by the core (D23).
    pub(crate) fn totp_setup(&self, input: &str, title: &str) -> Result<String> {
        Ok(pwm_client::normalize_totp(input, title)?
            .expose_secret()
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "id": "00000000-0000-4000-8000-000000000002",
            "v": 1, "type": "login", "title": "Mail", "notes": "n",
            "favorite": false, "folder_id": null, "tags": [],
            "custom_fields": [
                {"name": "PIN", "value": "1234", "kind": "hidden"},
                {"name": "Hint", "value": "blue", "kind": "text"}
            ],
            "created_at": 1, "updated_at": 2, "trashed_at": null,
            "login": {
                "username": "a@example.com", "password": "s3cret",
                "urls": [{"url": "https://mail.example.com", "match": "domain"}],
                "totp": "otpauth://totp/Mail?secret=JBSWY3DPEHPK3PXP",
                "passkeys": [{"credential_id": "AAAA", "rp_id": "example.com", "user_handle": "BBBB",
                              "user_name": "a", "private_key": "CCCC", "sign_count": 0, "created_at": 5}]
            },
            "history": [{"saved_at": 9, "v": 1, "type": "login", "title": "Mail",
                         "login": {"username": "a@example.com", "password": "older-pw-9"}}],
            "future_field": {"kept": true}
        })
    }

    #[test]
    fn the_hidden_places_are_the_secrets_that_exist() {
        assert_eq!(
            hidden_paths(&sample()),
            [
                "/login/password",
                "/login/totp",
                "/custom_fields/0/value",
                "/history/0/login/password"
            ]
        );
        let card =
            json!({"type": "card", "card": {"number": "4111 1111 1111 1234", "security_code": ""}});
        assert_eq!(hidden_paths(&card), ["/card/number"]);
    }

    #[test]
    fn the_detail_never_holds_a_hidden_value_or_a_passkey_key() {
        let shown = masked(&sample()).to_string();
        for secret in [
            "s3cret",
            "JBSWY3DPEHPK3PXP",
            "1234",
            "older-pw-9",
            "CCCC",
            "AAAA",
            "BBBB",
        ] {
            assert!(!shown.contains(secret), "{secret} in {shown}");
        }
        // Everything else stays, unknown fields included.
        assert!(shown.contains("blue") && shown.contains("future_field"));
        assert!(shown.contains("example.com"));
    }

    #[test]
    fn newer_items_are_read_only() {
        assert!(editable(&sample()));
        assert!(!editable(&json!({"v": 2, "type": "login"})));
        assert!(!editable(&json!({"v": 1, "type": "wifi"})));
    }

    #[test]
    fn the_list_line_never_shows_a_whole_card_number() {
        let card = json!({"type": "card", "card": {"number": "4111-1111-1111-1234"}});
        assert_eq!(subtitle(&card), "•••• 1234");
        let identity =
            json!({"type": "identity", "identity": {"first_name": "Ada", "last_name": "Lovelace"}});
        assert_eq!(subtitle(&identity), "Ada Lovelace");
    }
}
