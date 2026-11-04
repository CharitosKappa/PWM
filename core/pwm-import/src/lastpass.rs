//! LastPass CSV: `url,username,password,totp,extra,name,grouping,fav`
//! (older exports lack `totp`). Secure notes have the URL `http://sn`; a
//! structured note keeps its fields as `Key:value` lines after `NoteType:`,
//! and credit cards and addresses map to cards and identities.

use pwm_vault::{FieldKind, Item, UrlMatch};

use crate::build::{self, Builder};
use crate::csv::{self, Table};
use crate::{ImportError, Parsed};

pub fn is_lastpass(headers: &[String]) -> bool {
    let has = |n: &str| headers.iter().any(|h| h == n);
    has("url") && has("extra") && has("grouping") && has("name") && has("fav")
}

pub fn parse(bytes: &[u8], now_ms: i64) -> Result<Parsed, ImportError> {
    let table = csv::read(bytes)?;
    if !is_lastpass(&lower(&table.headers)) {
        return Err(ImportError::Unreadable);
    }
    let mut b = Builder::new(now_ms);
    for row in &table.rows {
        add(&mut b, &table, row)?;
    }
    Ok(b.finish())
}

fn lower(headers: &[String]) -> Vec<String> {
    headers
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect()
}

fn add(b: &mut Builder, t: &Table, row: &[String]) -> Result<(), ImportError> {
    let url = t.get(row, &["url"]);
    let name = t.get(row, &["name"]);
    let extra = t.get(row, &["extra"]);
    let folder = b.folder_path(t.get(row, &["grouping"]), '\\')?;
    let favorite = t.get(row, &["fav"]) == "1";
    let mut item = if url == "http://sn" {
        note(name, extra, b.now_ms)
    } else {
        let mut item = build::new_item(build::login_kind(), name, b.now_ms);
        if let Some(login) = build::login(&mut item) {
            login.username = t.get(row, &["username"]).to_owned();
            login.password = t.get(row, &["password"]).to_owned();
        }
        build::add_url(&mut item, url, UrlMatch::Domain);
        build::set_totp(&mut item, t.get(row, &["totp"]));
        build::add_note(&mut item, extra);
        item
    };
    item.folder_id = folder;
    item.favorite = favorite;
    b.push(item)
}

/// A secure note; a structured one becomes a card or an identity.
fn note(name: &str, extra: &str, now_ms: i64) -> Item {
    let Some(rest) = extra.strip_prefix("NoteType:") else {
        let mut item = build::new_item(pwm_vault::ItemKind::Note, name, now_ms);
        build::add_note(&mut item, extra);
        return item;
    };
    let mut lines = rest.split('\n');
    let kind = lines.next().unwrap_or("").trim();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut notes = String::new();
    // `Notes:` comes last and may span lines.
    let mut in_notes = false;
    for line in lines {
        if in_notes {
            notes.push('\n');
            notes.push_str(line);
        } else if let Some(value) = line.strip_prefix("Notes:") {
            notes = value.to_owned();
            in_notes = true;
        } else if let Some((key, value)) = line.split_once(':') {
            fields.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    };
    let mut used: Vec<&str> = vec!["Language"];
    let mut item = match kind {
        "Credit Card" => {
            let mut item = build::new_item(build::card_kind(), name, now_ms);
            if let Some(card) = build::card(&mut item) {
                card.cardholder = get("Name on Card").to_owned();
                card.number = get("Number").to_owned();
                card.security_code = get("Security Code").to_owned();
                card.brand = get("Type").to_owned();
                (card.expiry_month, card.expiry_year) = build::expiry(get("Expiration Date"));
            }
            used.extend([
                "Name on Card",
                "Number",
                "Security Code",
                "Type",
                "Expiration Date",
            ]);
            item
        }
        "Address" => {
            let mut item = build::new_item(build::identity_kind(), name, now_ms);
            if let Some(id) = build::identity(&mut item) {
                id.first_name = get("First Name").to_owned();
                id.middle_name = get("Middle Name").to_owned();
                id.last_name = get("Last Name").to_owned();
                id.company = get("Company").to_owned();
                id.email = get("Email Address").to_owned();
                id.phone = get("Phone").to_owned();
                id.address_lines = ["Address 1", "Address 2", "Address 3"]
                    .iter()
                    .map(|k| get(k).to_owned())
                    .filter(|l| !l.is_empty())
                    .collect();
                id.city = get("City / Town").to_owned();
                id.region = get("State").to_owned();
                id.postal_code = get("Zip / Postal Code").to_owned();
                id.country = get("Country").to_owned();
            }
            used.extend([
                "First Name",
                "Middle Name",
                "Last Name",
                "Company",
                "Email Address",
                "Phone",
                "Address 1",
                "Address 2",
                "Address 3",
                "City / Town",
                "State",
                "Zip / Postal Code",
                "Country",
            ]);
            item
        }
        _ => build::new_item(pwm_vault::ItemKind::Note, name, now_ms),
    };
    for (key, value) in &fields {
        if !used.contains(&key.as_str()) {
            let hidden = ["Password", "PIN", "Security Code", "Passphrase"]
                .iter()
                .any(|h| key.contains(h));
            let kind = if hidden {
                FieldKind::Hidden
            } else {
                FieldKind::Text
            };
            build::add_field(&mut item, key, value, kind);
        }
    }
    build::add_note(&mut item, &notes);
    item
}
