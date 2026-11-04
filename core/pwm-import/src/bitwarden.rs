//! Bitwarden JSON (D51 §4): unencrypted, or password-protected (see
//! `bitwarden_crypto`). The "account restricted" encrypted export needs the
//! Bitwarden account's own key, so it is refused with what to do instead.
//! Types: 1 login, 2 secure note, 3 card, 4 identity, 5 SSH key (left out).

use base64::Engine as _;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use pwm_crypto::Secret;
use pwm_vault::{FieldKind, Item, ItemKind, Passkey, UrlMatch};
use serde_json::Value;

use crate::build::{self, Builder, LeftOut};
use crate::{ImportError, Parsed, bitwarden_crypto, json};

pub fn parse(
    bytes: &[u8],
    password: Option<&Secret<String>>,
    now_ms: i64,
) -> Result<Parsed, ImportError> {
    let doc = json::parse(bytes)?;
    let root = doc.get();
    if json::truthy(root, "encrypted") {
        if !json::truthy(root, "passwordProtected") {
            return Err(ImportError::Unsupported(
                "import_bitwarden_account_restricted",
            ));
        }
        let password = password.ok_or(ImportError::PasswordNeeded)?;
        let inner = bitwarden_crypto::open(root, password.expose_secret())?;
        let inner = json::parse(&inner)?;
        if json::truthy(inner.get(), "encrypted") {
            return Err(ImportError::Unreadable);
        }
        return read(inner.get(), now_ms);
    }
    read(root, now_ms)
}

fn read(root: &Value, now_ms: i64) -> Result<Parsed, ImportError> {
    if root.get("items").is_none() {
        return Err(ImportError::Unreadable);
    }
    let mut b = Builder::new(now_ms);
    let mut folders: Vec<(String, Option<pwm_vault::ItemId>)> = Vec::new();
    for folder in json::arr(root, "folders") {
        let id = b.folder_path(&json::s(folder, "name"), '/')?;
        folders.push((json::s(folder, "id"), id));
    }
    for v in json::arr(root, "items") {
        let Some(mut item) = item(&mut b, v) else {
            continue;
        };
        let folder_id = json::s(v, "folderId");
        item.folder_id = folders
            .iter()
            .find(|(id, _)| *id == folder_id)
            .and_then(|(_, f)| *f);
        b.push(item)?;
    }
    Ok(b.finish())
}

fn item(b: &mut Builder, v: &Value) -> Option<Item> {
    let name = json::s(v, "name");
    let created = json::s(v, "creationDate");
    let created = iso_ms(&created).unwrap_or(b.now_ms);
    let mut item = match json::int(v, "type") {
        Some(1) => {
            let login = v.get("login").unwrap_or(&Value::Null);
            let mut item = build::new_item(build::login_kind(), &name, created);
            if let Some(l) = build::login(&mut item) {
                l.username = json::s(login, "username");
                l.password = json::s(login, "password");
            }
            for uri in json::arr(login, "uris") {
                build::add_url(&mut item, &json::s(uri, "uri"), url_match(uri.get("match")));
            }
            build::set_totp(&mut item, &json::s(login, "totp"));
            for credential in json::arr(login, "fido2Credentials") {
                match passkey(credential) {
                    Some(passkey) => {
                        if let Some(l) = build::login(&mut item) {
                            l.passkeys.push(passkey);
                        }
                    }
                    None => b.leave_out(LeftOut::Passkeys),
                }
            }
            item
        }
        Some(2) => build::new_item(ItemKind::Note, &name, created),
        Some(3) => {
            let c = v.get("card").unwrap_or(&Value::Null);
            let mut item = build::new_item(build::card_kind(), &name, created);
            if let Some(card) = build::card(&mut item) {
                card.cardholder = json::s(c, "cardholderName");
                card.number = json::s(c, "number");
                card.security_code = json::s(c, "code");
                card.brand = json::s(c, "brand");
                card.expiry_month = json::int(c, "expMonth")
                    .and_then(|m| u8::try_from(m).ok())
                    .filter(|m| (1..=12).contains(m));
                card.expiry_year = json::int(c, "expYear")
                    .and_then(|y| u16::try_from(y).ok())
                    .map(|y| if y < 100 { 2000 + y } else { y });
            }
            item
        }
        Some(4) => {
            let i = v.get("identity").unwrap_or(&Value::Null);
            let mut item = build::new_item(build::identity_kind(), &name, created);
            if let Some(id) = build::identity(&mut item) {
                id.first_name = json::s(i, "firstName");
                id.middle_name = json::s(i, "middleName");
                id.last_name = json::s(i, "lastName");
                id.email = json::s(i, "email");
                id.phone = json::s(i, "phone");
                id.company = json::s(i, "company");
                id.address_lines = ["address1", "address2", "address3"]
                    .iter()
                    .map(|k| json::s(i, k))
                    .filter(|l| !l.is_empty())
                    .collect();
                id.city = json::s(i, "city");
                id.region = json::s(i, "state");
                id.postal_code = json::s(i, "postalCode");
                id.country = json::s(i, "country");
            }
            for (key, label, kind) in [
                ("title", "Title", FieldKind::Text),
                ("username", "Username", FieldKind::Text),
                ("ssn", "Social security number", FieldKind::Hidden),
                ("passportNumber", "Passport number", FieldKind::Hidden),
                ("licenseNumber", "License number", FieldKind::Hidden),
            ] {
                build::add_field(&mut item, label, &json::s(i, key), kind);
            }
            item
        }
        Some(5) => {
            b.leave_out(LeftOut::SshKeys);
            return None;
        }
        _ => {
            b.leave_out(LeftOut::UnknownTypes);
            return None;
        }
    };
    item.updated_at = iso_ms(&json::s(v, "revisionDate")).unwrap_or(created);
    item.favorite = json::truthy(v, "favorite");
    build::add_note(&mut item, &json::s(v, "notes"));
    for field in json::arr(v, "fields") {
        let kind = match json::int(field, "type") {
            Some(1) => FieldKind::Hidden,
            // A linked field only points at another field.
            Some(3) => continue,
            _ => FieldKind::Text,
        };
        build::add_field(
            &mut item,
            &json::s(field, "name"),
            &json::s(field, "value"),
            kind,
        );
    }
    let history: Vec<(String, i64)> = json::arr(v, "passwordHistory")
        .iter()
        .map(|h| {
            let at = iso_ms(&json::s(h, "lastUsedDate")).unwrap_or(created);
            (json::s(h, "password"), at)
        })
        .collect();
    build::add_password_history(&mut item, history);
    if let Some(deleted) = iso_ms(&json::s(v, "deletedDate")) {
        build::trash(&mut item, deleted);
    }
    Some(item)
}

fn url_match(value: Option<&Value>) -> UrlMatch {
    match value.and_then(Value::as_i64) {
        Some(1) => UrlMatch::Host,
        Some(2) => UrlMatch::StartsWith,
        Some(3) => UrlMatch::Exact,
        Some(4) => UrlMatch::Regex,
        Some(5) => UrlMatch::Never,
        _ => UrlMatch::Domain,
    }
}

/// A passkey with its ES256 private key (PKCS#8), as Bitwarden exports them.
fn passkey(v: &Value) -> Option<Passkey> {
    if json::s(v, "keyAlgorithm") != "ECDSA" || json::s(v, "keyCurve") != "P-256" {
        return None;
    }
    let b64 = |s: &str| {
        let s = s.trim();
        URL_SAFE_NO_PAD
            .decode(s.trim_end_matches('='))
            .or_else(|_| URL_SAFE.decode(s))
            .ok()
    };
    let private_key = b64(&json::s(v, "keyValue")).filter(|k| !k.is_empty())?;
    let id = json::s(v, "credentialId");
    let credential_id = match id.strip_prefix("b64.") {
        Some(rest) => b64(rest)?,
        None => guid_bytes(&id)?,
    };
    let mut passkey = Passkey::default();
    passkey.credential_id = credential_id;
    passkey.rp_id = json::s(v, "rpId");
    passkey.user_handle = b64(&json::s(v, "userHandle")).unwrap_or_default();
    passkey.user_name = json::s(v, "userName");
    passkey.private_key = private_key;
    passkey.sign_count = json::int(v, "counter")
        .and_then(|c| u32::try_from(c).ok())
        .unwrap_or(0);
    passkey.created_at = iso_ms(&json::s(v, "creationDate")).unwrap_or(0);
    if passkey.rp_id.is_empty() {
        return None;
    }
    Some(passkey)
}

/// A GUID's 16 bytes, in the order written.
fn guid_bytes(text: &str) -> Option<Vec<u8>> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    (0..32)
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// An ISO 8601 UTC time (`2024-01-02T03:04:05.678Z`) in Unix ms.
pub fn iso_ms(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, time) = text.split_once('T')?;
    let mut d = date.split('-');
    let (year, month, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let time = time.trim_end_matches('Z');
    let time = time.split(['+']).next()?;
    let mut t = time.split(':');
    let (hour, minute): (i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    let seconds = t.next().unwrap_or("0");
    let (secs, frac) = seconds.split_once('.').unwrap_or((seconds, "0"));
    let secs: i64 = secs.parse().ok()?;
    let millis: i64 = format!("{frac:0<3}").get(..3)?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || secs > 60
    {
        return None;
    }
    // Days from 1970-01-01 (Howard Hinnant's civil-from-days, inverted).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + secs * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times() {
        assert_eq!(iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_ms("2024-01-02T03:04:05.678Z"), Some(1_704_164_645_678));
        assert_eq!(iso_ms("2000-03-01T00:00:00.5Z"), Some(951_868_800_500));
        assert_eq!(iso_ms("not a time"), None);
        assert_eq!(iso_ms(""), None);
    }

    #[test]
    fn guids() {
        assert_eq!(
            guid_bytes("00112233-4455-6677-8899-aabbccddeeff"),
            Some((0..16).map(|i| i * 0x11).collect())
        );
        assert_eq!(guid_bytes("short"), None);
    }
}
