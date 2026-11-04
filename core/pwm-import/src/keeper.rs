//! Keeper JSON (D51 §4): `records` with title, login, password, URL, notes
//! and `custom_fields`. Typed values use `$type` keys (`$oneTimeCode`,
//! `$paymentCard`, `$address`, `$text:Label`); the one-time code, payment
//! cards and addresses map, the rest become custom fields. Folders are
//! paths with `\` between levels, shared folders included.

use pwm_vault::{FieldKind, Item, ItemKind, UrlMatch};
use serde_json::{Map, Value};

use crate::build::{self, Builder, LeftOut};
use crate::{ImportError, Parsed, json};

pub fn parse(bytes: &[u8], now_ms: i64) -> Result<Parsed, ImportError> {
    let doc = json::parse(bytes)?;
    let root = doc.get();
    if root.get("records").is_none() {
        return Err(ImportError::Unreadable);
    }
    let mut b = Builder::new(now_ms);
    for record in json::arr(root, "records") {
        let Some(mut item) = item(&mut b, record) else {
            continue;
        };
        if let Some(folder) = json::arr(record, "folders").first() {
            let mut path: Vec<String> = Vec::new();
            for key in ["shared_folder", "folder"] {
                path.extend(json::s(folder, key).split('\\').map(str::to_owned));
            }
            item.folder_id = b.folder(&path)?;
        }
        b.push(item)?;
    }
    Ok(b.finish())
}

fn item(b: &mut Builder, v: &Value) -> Option<Item> {
    let title = json::s(v, "title");
    let custom = json::obj(v, "custom_fields");
    let card_value = custom.and_then(|c| typed(c, "$paymentCard"));
    let address = custom.and_then(|c| typed(c, "$address"));
    let kind = match json::s(v, "$type").as_str() {
        "file" | "photo" => {
            b.leave_out(LeftOut::Attachments);
            return None;
        }
        "sshKeys" => {
            b.leave_out(LeftOut::SshKeys);
            return None;
        }
        "bankCard" => build::card_kind(),
        "address" => build::identity_kind(),
        "encryptedNotes" => ItemKind::Note,
        "" | "login" | "legacy" => build::login_kind(),
        _ if card_value.is_some() => build::card_kind(),
        _ if json::s(v, "login").is_empty() && json::s(v, "password").is_empty() => ItemKind::Note,
        _ => build::login_kind(),
    };
    let mut item = build::new_item(kind, &title, b.now_ms);
    if let Some(login) = build::login(&mut item) {
        login.username = json::s(v, "login");
        login.password = json::s(v, "password");
    }
    build::add_url(&mut item, &json::s(v, "login_url"), UrlMatch::Domain);
    build::add_note(&mut item, &json::s(v, "notes"));
    if let Some(card_value) = card_value
        && let Some(card) = build::card(&mut item)
    {
        card.number = json::s(card_value, "cardNumber");
        card.security_code = json::s(card_value, "cardSecurityCode");
        (card.expiry_month, card.expiry_year) =
            build::expiry(&json::s(card_value, "cardExpirationDate"));
        card.cardholder = custom
            .and_then(|c| typed(c, "$text:cardholderName"))
            .map_or(String::new(), |v| json::text(Some(v)));
    }
    if let Some(address) = address
        && let Some(identity) = build::identity(&mut item)
    {
        identity.address_lines = [json::s(address, "street1"), json::s(address, "street2")]
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect();
        identity.city = json::s(address, "city");
        identity.region = json::s(address, "state");
        identity.postal_code = json::s(address, "zip");
        identity.country = json::s(address, "country");
    }
    for (key, value) in custom.into_iter().flatten() {
        let (kind, label) = match key.split_once(':') {
            Some((kind, label)) => (kind, label),
            None => (key.as_str(), key.as_str()),
        };
        match kind {
            "$oneTimeCode" => build::set_totp(&mut item, &json::text(Some(value))),
            // Mapped above.
            "$paymentCard" if matches!(item.kind, ItemKind::Card(_)) => {}
            "$address" if matches!(item.kind, ItemKind::Identity(_)) => {}
            "$text" if label == "cardholderName" && matches!(item.kind, ItemKind::Card(_)) => {}
            "$secret" | "$pinCode" => build::add_field(
                &mut item,
                label_of(label, kind),
                &flat(value),
                FieldKind::Hidden,
            ),
            "$url" => build::add_field(
                &mut item,
                label_of(label, kind),
                &flat(value),
                FieldKind::Url,
            ),
            "$date" | "$birthDate" | "$expirationDate" => build::add_field(
                &mut item,
                label_of(label, kind),
                &flat(value),
                FieldKind::Date,
            ),
            _ => build::add_field(
                &mut item,
                label_of(label, kind),
                &flat(value),
                FieldKind::Text,
            ),
        }
    }
    Some(item)
}

/// A typed value by its key, with or without a label (`$paymentCard`,
/// `$paymentCard:Label`).
fn typed<'a>(fields: &'a Map<String, Value>, kind: &str) -> Option<&'a Value> {
    fields
        .iter()
        .find(|(k, _)| {
            *k == kind
                || k.strip_prefix(kind)
                    .is_some_and(|rest| rest.starts_with(':'))
        })
        .map(|(_, v)| v)
}

/// `$text:Label` keeps `Label`; a bare `$pinCode` becomes `PIN code`.
fn label_of<'a>(label: &'a str, kind: &'a str) -> &'a str {
    if label != kind && !label.is_empty() {
        return label;
    }
    match kind {
        "$pinCode" => "PIN code",
        "$secret" => "Secret",
        "$url" => "Website",
        other => other.trim_start_matches('$'),
    }
}

/// A value as text; an object's values joined.
fn flat(value: &Value) -> String {
    match value {
        Value::Object(map) => map
            .values()
            .map(|v| json::text(Some(v)))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Array(items) => items
            .iter()
            .map(flat)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        other => json::text(Some(other)),
    }
}
