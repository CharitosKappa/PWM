//! Dashlane: the CSV export is a ZIP of tables (`credentials.csv`,
//! `securenotes.csv`, `payments.csv`, `personalInfo.csv`, `ids.csv`); a lone
//! `credentials.csv` is read too. The older JSON export has one list per
//! kind (`AUTHENTIFIANT`, `SECURENOTE`, `PAYMENTMEANS_CREDITCARD`, ...).

use pwm_vault::{FieldKind, Item, ItemKind, UrlMatch};
use serde_json::Value;

use crate::build::{self, Builder, LeftOut};
use crate::csv::{self, Table};
use crate::zipfile::{self, Zip};
use crate::{ImportError, Parsed, json};

pub fn is_credentials_csv(headers: &[String]) -> bool {
    let has = |n: &str| headers.iter().any(|h| h == n);
    has("username") && has("username2") && has("title") && has("password") && has("url")
}

pub fn parse(bytes: &[u8], now_ms: i64) -> Result<Parsed, ImportError> {
    let mut b = Builder::new(now_ms);
    if zipfile::is_zip(bytes) {
        let mut zip = Zip::open(bytes)?;
        let mut found = false;
        for (file, add) in [
            ("credentials.csv", credential as Adder),
            ("securenotes.csv", secure_note),
            ("payments.csv", payment),
            ("personalInfo.csv", personal_info),
            ("ids.csv", id_document),
        ] {
            if let Some(name) = zip.find(file) {
                found = true;
                let data = zip.read(&name)?;
                read_table(&mut b, &data, add)?;
            }
        }
        if !found {
            return Err(ImportError::Unreadable);
        }
        return Ok(b.finish());
    }
    let text = csv::strip_bom(bytes);
    if text.iter().copied().find(|c| !c.is_ascii_whitespace()) == Some(b'{') {
        parse_json(&mut b, text)?;
        return Ok(b.finish());
    }
    read_table(&mut b, text, credential)?;
    Ok(b.finish())
}

type Adder = fn(&mut Builder, &Table, &[String]) -> Result<(), ImportError>;

fn read_table(b: &mut Builder, data: &[u8], add: Adder) -> Result<(), ImportError> {
    let table = csv::read(data)?;
    for row in &table.rows {
        add(b, &table, row)?;
    }
    Ok(())
}

fn credential(b: &mut Builder, t: &Table, row: &[String]) -> Result<(), ImportError> {
    let get = |n: &[&str]| t.get(row, n);
    let mut item = build::new_item(build::login_kind(), get(&["title"]), b.now_ms);
    if let Some(login) = build::login(&mut item) {
        login.username = get(&["username"]).to_owned();
        login.password = get(&["password"]).to_owned();
    }
    build::add_url(&mut item, get(&["url"]), UrlMatch::Domain);
    build::add_field(
        &mut item,
        "Second username",
        get(&["username2"]),
        FieldKind::Text,
    );
    build::add_field(
        &mut item,
        "Third username",
        get(&["username3"]),
        FieldKind::Text,
    );
    build::set_totp(
        &mut item,
        get(&["otpUrl", "otpSecret", "otpurl", "otpsecret"]),
    );
    build::add_note(&mut item, get(&["note"]));
    item.folder_id = b.folder(&[get(&["category"])])?;
    b.push(item)
}

fn secure_note(b: &mut Builder, t: &Table, row: &[String]) -> Result<(), ImportError> {
    let mut item = build::new_item(ItemKind::Note, t.get(row, &["title"]), b.now_ms);
    build::add_note(&mut item, t.get(row, &["note"]));
    item.folder_id = b.folder(&[t.get(row, &["category"])])?;
    b.push(item)
}

fn payment(b: &mut Builder, t: &Table, row: &[String]) -> Result<(), ImportError> {
    let get = |n: &[&str]| t.get(row, n);
    let title = get(&["account_name", "name"]);
    if get(&["type"]) != "payment_card" && get(&["cc_number"]).is_empty() {
        // Bank accounts: a note with their fields.
        let mut item = build::new_item(ItemKind::Note, title, b.now_ms);
        build::add_field(
            &mut item,
            "Account holder",
            get(&["account_holder"]),
            FieldKind::Text,
        );
        build::add_field(
            &mut item,
            "Account number",
            get(&["account_number"]),
            FieldKind::Hidden,
        );
        build::add_field(
            &mut item,
            "Routing number",
            get(&["routing_number"]),
            FieldKind::Text,
        );
        build::add_note(&mut item, get(&["note"]));
        return b.push(item);
    }
    let mut item = build::new_item(build::card_kind(), title, b.now_ms);
    if let Some(card) = build::card(&mut item) {
        card.cardholder = get(&["account_holder"]).to_owned();
        card.number = get(&["cc_number"]).to_owned();
        card.security_code = get(&["code"]).to_owned();
        card.expiry_month = get(&["expiration_month"])
            .parse()
            .ok()
            .filter(|m| (1..=12).contains(m));
        card.expiry_year = get(&["expiration_year"])
            .parse::<u16>()
            .ok()
            .map(|y| if y < 100 { 2000 + y } else { y });
    }
    build::add_note(&mut item, get(&["note"]));
    b.push(item)
}

fn personal_info(b: &mut Builder, t: &Table, row: &[String]) -> Result<(), ImportError> {
    let get = |n: &[&str]| t.get(row, n);
    let title = match get(&["item_name"]) {
        "" => get(&["title", "type"]),
        name => name,
    };
    let mut item = build::new_item(build::identity_kind(), title, b.now_ms);
    if let Some(id) = build::identity(&mut item) {
        id.first_name = get(&["first_name"]).to_owned();
        id.middle_name = get(&["middle_name"]).to_owned();
        id.last_name = get(&["last_name"]).to_owned();
        id.email = get(&["email"]).to_owned();
        id.phone = get(&["phone_number"]).to_owned();
        id.address_lines = [get(&["address"])]
            .iter()
            .filter(|l| !l.is_empty())
            .map(|l| (*l).to_owned())
            .collect();
        id.city = get(&["city"]).to_owned();
        id.region = get(&["state"]).to_owned();
        id.postal_code = get(&["zip"]).to_owned();
        id.country = get(&["country"]).to_owned();
    }
    build::add_field(
        &mut item,
        "Date of birth",
        get(&["date_of_birth"]),
        FieldKind::Date,
    );
    build::add_field(&mut item, "Website", get(&["url"]), FieldKind::Url);
    b.push(item)
}

/// Passports, licences and other IDs: a note with the document's fields.
fn id_document(b: &mut Builder, t: &Table, row: &[String]) -> Result<(), ImportError> {
    let get = |n: &[&str]| t.get(row, n);
    let title = match get(&["name"]) {
        "" => get(&["type"]),
        name => name,
    };
    let mut item = build::new_item(ItemKind::Note, title, b.now_ms);
    build::add_field(&mut item, "Type", get(&["type"]), FieldKind::Text);
    build::add_field(&mut item, "Number", get(&["number"]), FieldKind::Hidden);
    build::add_field(&mut item, "Issued", get(&["issue_date"]), FieldKind::Date);
    build::add_field(
        &mut item,
        "Expires",
        get(&["expiration_date"]),
        FieldKind::Date,
    );
    build::add_field(
        &mut item,
        "Place of issue",
        get(&["place_of_issue"]),
        FieldKind::Text,
    );
    b.push(item)
}

fn parse_json(b: &mut Builder, bytes: &[u8]) -> Result<(), ImportError> {
    let doc = json::parse(bytes)?;
    let root = doc.get();
    if !root.is_object() {
        return Err(ImportError::Unreadable);
    }
    for v in json::arr(root, "AUTHENTIFIANT") {
        let mut item = build::new_item(build::login_kind(), &json::s(v, "title"), b.now_ms);
        let login_name = match json::s(v, "login").as_str() {
            "" => json::s(v, "email"),
            login => login.to_owned(),
        };
        if let Some(login) = build::login(&mut item) {
            login.username = login_name;
            login.password = json::s(v, "password");
        }
        build::add_url(&mut item, &json::s(v, "domain"), UrlMatch::Domain);
        build::add_field(
            &mut item,
            "Second username",
            &json::s(v, "secondaryLogin"),
            FieldKind::Text,
        );
        build::add_note(&mut item, &json::s(v, "note"));
        b.push(item)?;
    }
    for v in json::arr(root, "SECURENOTE") {
        let mut item = build::new_item(ItemKind::Note, &json::s(v, "title"), b.now_ms);
        build::add_note(&mut item, &json::s(v, "content"));
        b.push(item)?;
    }
    for v in json::arr(root, "PAYMENTMEANS_CREDITCARD") {
        b.push(json_card(v, b.now_ms))?;
    }
    for (key, value) in root.as_object().into_iter().flatten() {
        let known = ["AUTHENTIFIANT", "SECURENOTE", "PAYMENTMEANS_CREDITCARD"];
        if !known.contains(&key.as_str()) {
            let count = value.as_array().map_or(0, Vec::len);
            b.report.leave_out(LeftOut::UnknownTypes, count);
        }
    }
    Ok(())
}

fn json_card(v: &Value, now_ms: i64) -> Item {
    let title = match json::s(v, "name").as_str() {
        "" => json::s(v, "bank"),
        name => name.to_owned(),
    };
    let mut item = build::new_item(build::card_kind(), &title, now_ms);
    if let Some(card) = build::card(&mut item) {
        card.cardholder = json::s(v, "owner");
        card.number = json::s(v, "cardNumber");
        card.security_code = json::s(v, "securityCode");
        card.brand = json::s(v, "type");
        card.expiry_month = json::int(v, "expireMonth")
            .and_then(|m| u8::try_from(m).ok())
            .filter(|m| (1..=12).contains(m));
        card.expiry_year = json::int(v, "expireYear").and_then(|y| u16::try_from(y).ok());
    }
    item
}
