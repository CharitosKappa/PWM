//! NordPass CSV: one table for every type, chosen by the `type` column
//! (`password`, `credit_card`, `identity`, `note`, `folder`). Extra URLs and
//! custom fields are JSON inside their cells.

use crate::build::{self, Builder, LeftOut};
use crate::csv::{self, Table};
use crate::{ImportError, Parsed, json};
use pwm_vault::{FieldKind, ItemKind, UrlMatch};

pub fn is_nordpass(headers: &[String]) -> bool {
    let has = |n: &str| headers.iter().any(|h| h == n);
    has("cardholdername") && has("additional_urls") && has("type")
}

pub fn parse(bytes: &[u8], now_ms: i64) -> Result<Parsed, ImportError> {
    let table = csv::read(bytes)?;
    let headers: Vec<String> = table
        .headers
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    if !is_nordpass(&headers) {
        return Err(ImportError::Unreadable);
    }
    let mut b = Builder::new(now_ms);
    // Folders first, so empty ones come too.
    for row in &table.rows {
        if table.get(row, &["type"]) == "folder" {
            b.folder(&[table.get(row, &["name"])])?;
        }
    }
    for row in &table.rows {
        add(&mut b, &table, row)?;
    }
    Ok(b.finish())
}

fn add(b: &mut Builder, t: &Table, row: &[String]) -> Result<(), ImportError> {
    let get = |names: &[&str]| t.get(row, names);
    let name = get(&["name"]);
    let mut item = match get(&["type"]) {
        "folder" => return Ok(()),
        "password" | "" => {
            let mut item = build::new_item(build::login_kind(), name, b.now_ms);
            if let Some(login) = build::login(&mut item) {
                login.username = get(&["username"]).to_owned();
                login.password = get(&["password"]).to_owned();
            }
            build::add_url(&mut item, get(&["url"]), UrlMatch::Domain);
            if let Ok(doc) = json::parse(get(&["additional_urls"]).as_bytes())
                && let Some(urls) = doc.get().as_array()
            {
                for url in urls {
                    build::add_url(&mut item, &json::text(Some(url)), UrlMatch::Domain);
                }
            }
            item
        }
        "credit_card" => {
            let mut item = build::new_item(build::card_kind(), name, b.now_ms);
            if let Some(card) = build::card(&mut item) {
                card.cardholder = get(&["cardholdername"]).to_owned();
                card.number = get(&["cardnumber"]).to_owned();
                card.security_code = get(&["cvc"]).to_owned();
                (card.expiry_month, card.expiry_year) = build::expiry(get(&["expirydate"]));
            }
            build::add_field(&mut item, "PIN", get(&["pin"]), FieldKind::Hidden);
            build::add_field(&mut item, "ZIP code", get(&["zipcode"]), FieldKind::Text);
            item
        }
        "identity" => {
            let mut item = build::new_item(build::identity_kind(), name, b.now_ms);
            if let Some(id) = build::identity(&mut item) {
                let full = get(&["full_name"]);
                match full.split_once(' ') {
                    Some((first, last)) => {
                        id.first_name = first.to_owned();
                        id.last_name = last.trim().to_owned();
                    }
                    None => id.first_name = full.to_owned(),
                }
                id.email = get(&["email"]).to_owned();
                id.phone = get(&["phone_number"]).to_owned();
                id.address_lines = [get(&["address1"]), get(&["address2"])]
                    .iter()
                    .filter(|l| !l.is_empty())
                    .map(|l| (*l).to_owned())
                    .collect();
                id.city = get(&["city"]).to_owned();
                id.region = get(&["state"]).to_owned();
                id.postal_code = get(&["zipcode"]).to_owned();
                id.country = get(&["country"]).to_owned();
            }
            item
        }
        "note" => build::new_item(ItemKind::Note, name, b.now_ms),
        _ => {
            b.leave_out(LeftOut::UnknownTypes);
            return Ok(());
        }
    };
    build::add_note(&mut item, get(&["note"]));
    custom_fields(&mut item, get(&["custom_fields"]));
    item.folder_id = b.folder(&[get(&["folder"])])?;
    b.push(item)
}

/// `[{"label": "...", "type": "text" | "hidden" | "totp" | "date", "value": "..."}]`
fn custom_fields(item: &mut pwm_vault::Item, cell: &str) {
    let Ok(doc) = json::parse(cell.as_bytes()) else {
        return;
    };
    let Some(fields) = doc.get().as_array() else {
        return;
    };
    for field in fields {
        let label = json::s(field, "label");
        let value = json::s(field, "value");
        match json::s(field, "type").as_str() {
            "totp" => build::set_totp(item, &value),
            "hidden" => build::add_field(item, &label, &value, FieldKind::Hidden),
            "date" => build::add_field(item, &label, &value, FieldKind::Date),
            _ => build::add_field(item, &label, &value, FieldKind::Text),
        }
    }
}
