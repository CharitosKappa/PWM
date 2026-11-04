//! 1Password `.1pux` (D51 §4): a ZIP whose `export.data` is JSON with
//! accounts, vaults and items. Logins, passwords, notes, cards and
//! identities map; other categories become notes with their fields;
//! documents and their files are left out; archived items come in trashed.
//! With more than one vault, each becomes a folder.

use pwm_vault::{FieldKind, Item, ItemKind, UrlMatch};
use serde_json::Value;

use crate::build::{self, Builder, LeftOut};
use crate::zipfile::Zip;
use crate::{ImportError, Parsed, json};

const LOGIN: &str = "001";
const CARD: &str = "002";
const IDENTITY: &str = "004";
const PASSWORD: &str = "005";
const DOCUMENT: &str = "006";
const SSH_KEY: &str = "114";

pub fn parse(bytes: &[u8], now_ms: i64) -> Result<Parsed, ImportError> {
    let mut zip = Zip::open(bytes)?;
    let name = zip.find("export.data").ok_or(ImportError::Unreadable)?;
    let data = zip.read(&name)?;
    let doc = json::parse(&data)?;
    let root = doc.get();
    let accounts = json::arr(root, "accounts");
    if accounts.is_empty() {
        return Err(ImportError::Unreadable);
    }
    let vaults: Vec<&Value> = accounts
        .iter()
        .flat_map(|a| json::arr(a, "vaults"))
        .collect();
    let mut b = Builder::new(now_ms);
    let one_vault = vaults.len() <= 1;
    for vault in vaults {
        let vault_folder = if one_vault {
            None
        } else {
            let attrs = vault.get("attrs").unwrap_or(&Value::Null);
            b.folder(&[json::s(attrs, "name")])?
        };
        for v in json::arr(vault, "items") {
            let Some(mut item) = item(&mut b, v) else {
                continue;
            };
            item.folder_id = vault_folder;
            b.push(item)?;
        }
    }
    Ok(b.finish())
}

fn item(b: &mut Builder, v: &Value) -> Option<Item> {
    let overview = v.get("overview").unwrap_or(&Value::Null);
    let details = v.get("details").unwrap_or(&Value::Null);
    let title = json::s(overview, "title");
    let created = json::int(v, "createdAt").map_or(b.now_ms, build::ms_from);
    let category = json::s(v, "categoryUuid");
    let kind = match category.as_str() {
        LOGIN | PASSWORD => build::login_kind(),
        CARD => build::card_kind(),
        IDENTITY => build::identity_kind(),
        DOCUMENT => {
            b.leave_out(LeftOut::Attachments);
            return None;
        }
        SSH_KEY => {
            b.leave_out(LeftOut::SshKeys);
            return None;
        }
        _ => ItemKind::Note,
    };
    let mut item = build::new_item(kind, &title, created);
    item.updated_at = json::int(v, "updatedAt").map_or(created, build::ms_from);
    item.favorite = json::int(v, "favIndex").is_some_and(|f| f > 0);
    item.tags = json::arr(overview, "tags")
        .iter()
        .map(|t| json::text(Some(t)))
        .filter(|t| !t.is_empty())
        .collect();
    // URLs: the list, or the single `url`.
    let urls = json::arr(overview, "urls");
    if urls.is_empty() {
        build::add_url(&mut item, &json::s(overview, "url"), UrlMatch::Domain);
    }
    for url in urls {
        build::add_url(&mut item, &json::s(url, "url"), UrlMatch::Domain);
    }
    for field in json::arr(details, "loginFields") {
        let value = json::s(field, "value");
        match json::s(field, "designation").as_str() {
            "username" => {
                if let Some(login) = build::login(&mut item) {
                    login.username = value;
                }
            }
            "password" => {
                if let Some(login) = build::login(&mut item) {
                    login.password = value;
                }
            }
            _ => {
                let kind = if json::s(field, "fieldType") == "P" {
                    FieldKind::Hidden
                } else {
                    FieldKind::Text
                };
                build::add_field(&mut item, &json::s(field, "name"), &value, kind);
            }
        }
    }
    // A "Password" item has its password in `details.password`.
    let password = json::s(details, "password");
    if let Some(login) = build::login(&mut item)
        && login.password.is_empty()
    {
        login.password = password;
    }
    let mut files = 0;
    for section in json::arr(details, "sections") {
        for field in json::arr(section, "fields") {
            files += usize::from(section_field(&mut item, field));
        }
    }
    b.report.leave_out(LeftOut::Attachments, files);
    build::add_note(&mut item, &json::s(details, "notesPlain"));
    let history: Vec<(String, i64)> = json::arr(details, "passwordHistory")
        .iter()
        .map(|h| {
            (
                json::s(h, "value"),
                json::int(h, "time").map_or(created, build::ms_from),
            )
        })
        .collect();
    build::add_password_history(&mut item, history);
    if json::s(v, "state") == "archived" {
        let at = item.updated_at;
        build::trash(&mut item, at);
    }
    Some(item)
}

/// One section field: `value` is an object whose single key says its type.
/// True for a file, whose content isn't in `export.data`.
fn section_field(item: &mut Item, field: &Value) -> bool {
    let id = json::s(field, "id");
    let label = match json::s(field, "title").as_str() {
        "" => id.clone(),
        t => t.to_owned(),
    };
    let Some((kind, value)) = field
        .get("value")
        .and_then(Value::as_object)
        .and_then(|o| o.iter().next())
    else {
        return false;
    };
    if kind == "file" {
        return true;
    }
    let text = json::text(Some(value));
    // Card and identity fields by their ids.
    if let Some(card) = build::card(item) {
        match (id.as_str(), kind.as_str()) {
            ("cardholder", _) => {
                card.cardholder = text;
                return false;
            }
            ("ccnum", _) | (_, "creditCardNumber") => {
                card.number = text;
                return false;
            }
            ("cvv", _) => {
                card.security_code = text;
                return false;
            }
            ("type", _) | (_, "creditCardType") => {
                card.brand = text;
                return false;
            }
            ("expiry", "monthYear") => {
                // `202705`
                (card.expiry_month, card.expiry_year) = build::expiry(&text);
                return false;
            }
            _ => {}
        }
    }
    if let Some(identity) = build::identity(item) {
        match id.as_str() {
            "firstname" => identity.first_name = text,
            "initial" => identity.middle_name = text,
            "lastname" => identity.last_name = text,
            "company" => identity.company = text,
            "email" => {
                identity.email = value
                    .as_object()
                    .map_or(text, |o| json::text(o.get("email_address")));
            }
            "defphone" | "cellphone" if identity.phone.is_empty() => identity.phone = text,
            "address" => {
                if let Some(address) = value.as_object() {
                    let get = |k: &str| json::text(address.get(k));
                    identity.address_lines = vec![get("street")]
                        .into_iter()
                        .filter(|l| !l.is_empty())
                        .collect();
                    identity.city = get("city");
                    identity.region = get("state");
                    identity.postal_code = get("zip");
                    identity.country = get("country");
                }
            }
            _ => return other_field(item, kind, &label, &text, value),
        }
        return false;
    }
    other_field(item, kind, &label, &text, value)
}

fn other_field(item: &mut Item, kind: &str, label: &str, text: &str, value: &Value) -> bool {
    match kind {
        "totp" => build::set_totp(item, text),
        "concealed" => build::add_field(item, label, text, FieldKind::Hidden),
        "url" => build::add_field(item, label, text, FieldKind::Url),
        "date" => build::add_field(item, label, text, FieldKind::Date),
        "email" => {
            let email = value
                .as_object()
                .map_or(text.to_owned(), |o| json::text(o.get("email_address")));
            build::add_field(item, label, &email, FieldKind::Text);
        }
        "address" => {
            if let Some(address) = value.as_object() {
                let parts: Vec<String> = ["street", "city", "state", "zip", "country"]
                    .iter()
                    .map(|k| json::text(address.get(*k)))
                    .filter(|p| !p.is_empty())
                    .collect();
                build::add_field(item, label, &parts.join(", "), FieldKind::Text);
            }
        }
        _ => build::add_field(item, label, text, FieldKind::Text),
    }
    false
}
