//! Proton Pass (D51 §4): the unencrypted ZIP's `data.json`, or that file on
//! its own. Vaults hold items of type login, note, creditCard and identity;
//! aliases are left out (they live in Proton's service), and so are
//! passkeys, whose content Proton doesn't document. The PGP-encrypted export
//! is refused with what to do instead. With more than one vault, each
//! becomes a folder.

use pwm_vault::{FieldKind, Item, ItemKind, UrlMatch};
use serde_json::Value;

use crate::build::{self, Builder, LeftOut};
use crate::zipfile::{self, Zip};
use crate::{ImportError, Parsed, json};

/// `state` 2 is Proton's trash.
const TRASHED: i64 = 2;

pub fn parse(bytes: &[u8], now_ms: i64) -> Result<Parsed, ImportError> {
    let data = if zipfile::is_zip(bytes) {
        let mut zip = Zip::open(bytes)?;
        if zip.names().iter().any(|n| n.ends_with(".pgp")) {
            return Err(ImportError::Unsupported("import_proton_encrypted"));
        }
        let name = zip.find("data.json").ok_or(ImportError::Unreadable)?;
        zip.read(&name)?
    } else {
        if bytes.starts_with(b"-----BEGIN PGP") {
            return Err(ImportError::Unsupported("import_proton_encrypted"));
        }
        zeroize::Zeroizing::new(bytes.to_vec())
    };
    let doc = json::parse(&data)?;
    let root = doc.get();
    if json::truthy(root, "encrypted") {
        return Err(ImportError::Unsupported("import_proton_encrypted"));
    }
    let vaults = json::obj(root, "vaults").ok_or(ImportError::Unreadable)?;
    let mut b = Builder::new(now_ms);
    let one_vault = vaults.len() <= 1;
    for vault in vaults.values() {
        let folder = if one_vault {
            None
        } else {
            b.folder(&[json::s(vault, "name")])?
        };
        for v in json::arr(vault, "items") {
            let Some(mut item) = item(&mut b, v) else {
                continue;
            };
            item.folder_id = folder;
            b.push(item)?;
        }
    }
    Ok(b.finish())
}

fn item(b: &mut Builder, v: &Value) -> Option<Item> {
    let data = v.get("data").unwrap_or(&Value::Null);
    let metadata = data.get("metadata").unwrap_or(&Value::Null);
    let content = data.get("content").unwrap_or(&Value::Null);
    let title = json::s(metadata, "name");
    let created = json::int(v, "createTime").map_or(b.now_ms, build::ms_from);
    let kind = match json::s(data, "type").as_str() {
        "login" => build::login_kind(),
        "note" => ItemKind::Note,
        "creditCard" => build::card_kind(),
        "identity" => build::identity_kind(),
        "alias" => {
            b.leave_out(LeftOut::Aliases);
            return None;
        }
        _ => {
            b.leave_out(LeftOut::UnknownTypes);
            return None;
        }
    };
    let mut item = build::new_item(kind, &title, created);
    item.updated_at = json::int(v, "modifyTime").map_or(created, build::ms_from);
    item.favorite = json::truthy(v, "pinned");
    build::add_note(&mut item, &json::s(metadata, "note"));
    if let Some(login) = build::login(&mut item) {
        let username = json::s(content, "itemUsername");
        let email = json::s(content, "itemEmail");
        login.username = if username.is_empty() {
            email.clone()
        } else {
            username.clone()
        };
        login.password = json::s(content, "password");
        if !username.is_empty() && !email.is_empty() {
            build::add_field(&mut item, "Email", &email, FieldKind::Text);
        }
        for url in json::arr(content, "urls") {
            build::add_url(&mut item, &json::text(Some(url)), UrlMatch::Domain);
        }
        build::set_totp(&mut item, &json::s(content, "totpUri"));
        b.report
            .leave_out(LeftOut::Passkeys, json::arr(content, "passkeys").len());
    }
    if let Some(card) = build::card(&mut item) {
        card.cardholder = json::s(content, "cardholderName");
        card.number = json::s(content, "number");
        card.security_code = json::s(content, "verificationNumber");
        (card.expiry_month, card.expiry_year) = build::expiry(&json::s(content, "expirationDate"));
        let pin = json::s(content, "pin");
        build::add_field(&mut item, "PIN", &pin, FieldKind::Hidden);
    }
    if let Some(identity) = build::identity(&mut item) {
        identity.first_name = json::s(content, "firstName");
        identity.middle_name = json::s(content, "middleName");
        identity.last_name = json::s(content, "lastName");
        if identity.first_name.is_empty() && identity.last_name.is_empty() {
            identity.first_name = json::s(content, "fullName");
        }
        identity.email = json::s(content, "email");
        identity.phone = json::s(content, "phoneNumber");
        identity.company = json::s(content, "company");
        identity.address_lines = [json::s(content, "streetAddress")]
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect();
        identity.city = json::s(content, "city");
        identity.region = json::s(content, "stateOrProvince");
        identity.postal_code = json::s(content, "zipOrPostalCode");
        identity.country = json::s(content, "countryOrRegion");
    }
    for field in json::arr(data, "extraFields") {
        let name = json::s(field, "fieldName");
        let value = field
            .get("data")
            .map_or(String::new(), |d| json::s(d, "content"));
        match json::s(field, "type").as_str() {
            "totp" => build::set_totp(&mut item, &value),
            "hidden" => build::add_field(&mut item, &name, &value, FieldKind::Hidden),
            "timestamp" => build::add_field(&mut item, &name, &value, FieldKind::Date),
            _ => build::add_field(&mut item, &name, &value, FieldKind::Text),
        }
    }
    if json::int(v, "state") == Some(TRASHED) {
        let at = item.updated_at;
        build::trash(&mut item, at);
    }
    Some(item)
}
