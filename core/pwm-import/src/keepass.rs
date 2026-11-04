//! KeePass `.kdbx`, versions 3.1 and 4 (D51 §5), read with the `keepass`
//! crate after our own header check: the key-derivation settings a file
//! asks for are bounded before any of it runs, so a crafted file can't
//! stall or exhaust a device. KeePass 1 (`.kdb`) is refused.
//!
//! Groups become nested folders and the recycle bin comes in trashed. Title,
//! username, password, URL, notes and tags map; the `otp` field and KeePass
//! 2's `TimeOtp-*` fields become the one-time code; other fields become
//! custom fields (protected ones hidden); entry history becomes password
//! history. Attachments are left out.

use keepass::db::{EntryRef, GroupRef};
use keepass::{Database, DatabaseKey};
use pwm_crypto::Secret;
use pwm_vault::{FieldKind, Item, ItemKind, UrlMatch};

use crate::build::{self, Builder, LeftOut};
use crate::{ImportError, Parsed};

const SIG1: [u8; 4] = [0x03, 0xD9, 0xA2, 0x9A];
const SIG2_KDBX: [u8; 4] = [0x67, 0xFB, 0x4B, 0xB5];
const SIG2_KDB: [u8; 4] = [0x65, 0xFB, 0x4B, 0xB5];

/// The most a file may ask for (D51 §5).
pub const MAX_AES_ROUNDS: u64 = 100_000_000;
pub const MAX_ARGON2_MEMORY: u64 = 1024 * 1024 * 1024;
pub const MAX_ARGON2_ITERATIONS: u64 = 100;
pub const MAX_ARGON2_PARALLELISM: u64 = 64;

/// KDF ids in KDBX 4's parameters.
const AES_KDF: [u8; 16] = [
    0xC9, 0xD9, 0xF3, 0x9A, 0x62, 0x8A, 0x44, 0x60, 0xBF, 0x74, 0x0D, 0x08, 0xC1, 0x8A, 0x4F, 0xEA,
];
const ARGON2D: [u8; 16] = [
    0xEF, 0x63, 0x6D, 0xDF, 0x8C, 0x29, 0x44, 0x4B, 0x91, 0xF7, 0xA9, 0xA4, 0x03, 0xE3, 0x0A, 0x0C,
];
const ARGON2ID: [u8; 16] = [
    0x9E, 0x29, 0x8B, 0x19, 0x56, 0xDB, 0x47, 0x73, 0xB2, 0x3D, 0xFC, 0x3E, 0xC6, 0xF0, 0xA1, 0xE6,
];

pub fn is_kdbx(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes[..4] == SIG1 && (bytes[4..8] == SIG2_KDBX || bytes[4..8] == SIG2_KDB)
}

/// Reads the outer header without decrypting anything and checks the key
/// derivation it asks for.
pub fn check_header(bytes: &[u8]) -> Result<(), ImportError> {
    if bytes.len() < 12 || bytes[..4] != SIG1 {
        return Err(ImportError::Unreadable);
    }
    if bytes[4..8] == SIG2_KDB {
        return Err(ImportError::Unsupported("import_keepass_kdb"));
    }
    if bytes[4..8] != SIG2_KDBX {
        return Err(ImportError::Unreadable);
    }
    let major = u16::from_le_bytes([bytes[10], bytes[11]]);
    let wide = match major {
        3 => false,
        4 => true,
        _ => return Err(ImportError::Unsupported("import_keepass_version")),
    };
    let mut at = 12;
    loop {
        let id = *bytes.get(at).ok_or(ImportError::Unreadable)?;
        at += 1;
        let len = if wide {
            let b = bytes.get(at..at + 4).ok_or(ImportError::Unreadable)?;
            at += 4;
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize
        } else {
            let b = bytes.get(at..at + 2).ok_or(ImportError::Unreadable)?;
            at += 2;
            u16::from_le_bytes([b[0], b[1]]) as usize
        };
        let value = bytes
            .get(at..at.checked_add(len).ok_or(ImportError::Unreadable)?)
            .ok_or(ImportError::Unreadable)?;
        at += len;
        match (id, wide) {
            (0, _) => return Ok(()),
            // KDBX 3: AES-KDF rounds.
            (6, false) => {
                let rounds =
                    u64::from_le_bytes(value.try_into().map_err(|_| ImportError::Unreadable)?);
                if rounds > MAX_AES_ROUNDS {
                    return Err(ImportError::Unsupported("import_keepass_too_costly"));
                }
            }
            // KDBX 4: the KDF parameters.
            (11, true) => check_kdf_parameters(value)?,
            _ => {}
        }
    }
}

/// KDBX 4's KDF parameters, a "variant dictionary" of typed values.
fn check_kdf_parameters(dict: &[u8]) -> Result<(), ImportError> {
    let bad = || ImportError::Unreadable;
    if dict.len() < 2 || dict[1] != 0x01 {
        return Err(bad());
    }
    let (mut uuid, mut rounds, mut memory, mut iterations, mut parallelism) =
        (None, 0u64, 0u64, 0u64, 0u64);
    let mut at = 2;
    loop {
        let kind = *dict.get(at).ok_or_else(bad)?;
        at += 1;
        if kind == 0 {
            break;
        }
        let read_len = |at: &mut usize| -> Result<usize, ImportError> {
            let b = dict.get(*at..*at + 4).ok_or_else(bad)?;
            *at += 4;
            usize::try_from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])).map_err(|_| bad())
        };
        let key_len = read_len(&mut at)?;
        let key = dict.get(at..at + key_len).ok_or_else(bad)?;
        at += key_len;
        let value_len = read_len(&mut at)?;
        let value = dict.get(at..at + value_len).ok_or_else(bad)?;
        at += value_len;
        let number = || -> u64 {
            let mut b = [0u8; 8];
            let n = value.len().min(8);
            b[..n].copy_from_slice(&value[..n]);
            u64::from_le_bytes(b)
        };
        match key {
            b"$UUID" => uuid = Some(value.to_vec()),
            b"R" => rounds = number(),
            b"M" => memory = number(),
            b"I" => iterations = number(),
            b"P" => parallelism = number(),
            _ => {}
        }
    }
    let too_costly = Err(ImportError::Unsupported("import_keepass_too_costly"));
    match uuid.as_deref() {
        Some(id) if id == AES_KDF => {
            if rounds > MAX_AES_ROUNDS {
                return too_costly;
            }
        }
        Some(id) if id == ARGON2D || id == ARGON2ID => {
            if memory > MAX_ARGON2_MEMORY
                || iterations > MAX_ARGON2_ITERATIONS
                || parallelism > MAX_ARGON2_PARALLELISM
            {
                return too_costly;
            }
        }
        _ => return Err(ImportError::Unsupported("import_keepass_version")),
    }
    Ok(())
}

/// Opens the database with its password, key file, or both.
pub fn parse(
    bytes: &[u8],
    password: Option<&Secret<String>>,
    key_file: Option<&Secret<Vec<u8>>>,
    now_ms: i64,
) -> Result<Parsed, ImportError> {
    check_header(bytes)?;
    if password.is_none() && key_file.is_none() {
        return Err(ImportError::PasswordNeeded);
    }
    let mut key = DatabaseKey::new();
    if let Some(password) = password {
        key = key.with_password(password.expose_secret());
    }
    if let Some(file) = key_file {
        key = key
            .with_keyfile(&mut file.expose_secret().as_slice())
            .map_err(|_| ImportError::Unreadable)?;
    }
    let db = Database::parse(bytes, key).map_err(|err| match err {
        keepass::db::DatabaseOpenError::Key(_) => ImportError::WrongPassword,
        _ => ImportError::Unreadable,
    })?;
    let mut b = Builder::new(now_ms);
    let bin = db.recycle_bin().map(|g| g.id());
    walk(&mut b, db.root(), &[], false, bin)?;
    Ok(b.finish())
}

/// Entries of `group` and its subgroups. The root group itself is no
/// folder; the recycle bin and what is in it come in trashed, folderless.
fn walk(
    b: &mut Builder,
    group: GroupRef<'_>,
    path: &[String],
    trashed: bool,
    bin: Option<keepass::db::GroupId>,
) -> Result<(), ImportError> {
    let folder = if trashed { None } else { b.folder(path)? };
    for entry in group.entries() {
        if let Some(mut item) = item(b, &entry) {
            item.folder_id = folder;
            if trashed {
                build::trash(&mut item, b.now_ms);
            }
            b.push(item)?;
        }
    }
    for child in group.groups() {
        let in_bin = trashed || Some(child.id()) == bin;
        let mut child_path = path.to_vec();
        child_path.push(child.name.clone());
        walk(b, child, &child_path, in_bin, bin)?;
    }
    Ok(())
}

const STANDARD: [&str; 5] = ["Title", "UserName", "Password", "URL", "Notes"];

fn item(b: &mut Builder, entry: &EntryRef<'_>) -> Option<Item> {
    let get = |key: &str| entry.get(key).unwrap_or("").trim().to_owned();
    let (title, username, password, url, notes) = (
        get("Title"),
        get("UserName"),
        get("Password"),
        get("URL"),
        get("Notes"),
    );
    let attachments = entry.attachments().count();
    b.report.leave_out(LeftOut::Attachments, attachments);
    let custom: Vec<(&String, &keepass::db::Value<String>)> = entry
        .fields
        .iter()
        .filter(|(k, _)| !STANDARD.contains(&k.as_str()))
        .collect();
    if [&title, &username, &password, &url, &notes]
        .iter()
        .all(|v| v.is_empty())
        && custom.is_empty()
    {
        b.leave_out(LeftOut::Empty);
        return None;
    }
    let created = entry
        .times
        .creation
        .map_or(b.now_ms, |t| t.and_utc().timestamp_millis());
    let kind = if username.is_empty() && password.is_empty() && url.is_empty() && custom.is_empty()
    {
        ItemKind::Note
    } else {
        build::login_kind()
    };
    let mut item = build::new_item(kind, &title, created);
    item.updated_at = entry
        .times
        .last_modification
        .map_or(created, |t| t.and_utc().timestamp_millis());
    item.tags = entry
        .tags
        .iter()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    if let Some(login) = build::login(&mut item) {
        login.username = username;
        login.password = password.clone();
    }
    build::add_url(&mut item, &url, UrlMatch::Domain);
    build::add_note(&mut item, &notes);
    // The one-time code: KeePassXC's `otp` URI, or KeePass 2's fields.
    let otp = entry
        .get("otp")
        .map(str::to_owned)
        .or_else(|| time_otp(entry, &item.title));
    if let Some(otp) = otp {
        build::set_totp(&mut item, &otp);
    }
    let mut sorted = custom;
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (key, value) in sorted {
        if key == "otp" || key.starts_with("TimeOtp-") {
            continue;
        }
        // KeePassXC and Keepass2Android keep extra URLs as KP2A_URL*.
        if key.starts_with("KP2A_URL") {
            build::add_url(&mut item, value.get(), UrlMatch::Domain);
            continue;
        }
        let kind = if value.is_protected() {
            FieldKind::Hidden
        } else {
            FieldKind::Text
        };
        build::add_field(&mut item, key, value.get(), kind);
    }
    // Earlier versions, as password history.
    if let Some(history) = &entry.history {
        let old: Vec<(String, i64)> = history
            .get_entries()
            .iter()
            .filter_map(|e| {
                let p = e.get("Password")?.to_owned();
                let at = e
                    .times
                    .last_modification
                    .map_or(created, |t| t.and_utc().timestamp_millis());
                (p != password).then_some((p, at))
            })
            .collect();
        build::add_password_history(&mut item, old);
    }
    Some(item)
}

/// KeePass 2.47's built-in TOTP fields as an `otpauth://` URI. Only the
/// base32 secret is read; the other encodings stay as hidden fields.
fn time_otp(entry: &EntryRef<'_>, title: &str) -> Option<String> {
    let secret = entry.get("TimeOtp-Secret-Base32")?.trim().replace(' ', "");
    if secret.is_empty() {
        return None;
    }
    let mut uri = format!(
        "otpauth://totp/{}?secret={secret}",
        url::form_urlencoded::byte_serialize(title.as_bytes()).collect::<String>()
    );
    if let Some(digits) = entry
        .get("TimeOtp-Length")
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        uri.push_str(&format!("&digits={digits}"));
    }
    if let Some(period) = entry
        .get("TimeOtp-Period")
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        uri.push_str(&format!("&period={period}"));
    }
    match entry.get("TimeOtp-Algorithm").map(str::trim) {
        Some("HMAC-SHA-256") => uri.push_str("&algorithm=SHA256"),
        Some("HMAC-SHA-512") => uri.push_str("&algorithm=SHA512"),
        _ => {}
    }
    Some(uri)
}
