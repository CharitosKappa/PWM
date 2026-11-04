//! KeePass `.kdbx` (D51 §5): databases made here with the `keepass` crate's
//! KDBX 4 writer, KDBX 3.1 files from `fixtures/keepass/` when present, and
//! our header check against key settings a device can't afford.

use keepass::config::KdfConfig;
use keepass::db::{Entry, Value, fields};
use keepass::{Database, DatabaseKey};
use pwm_crypto::Secret;
use pwm_import::{Detected, Format, ImportError, LeftOut, Options, Parsed, detect, parse};
use pwm_vault::{FieldKind, Item, ItemId, ItemKind};

const NOW: i64 = 1_767_225_600_000;
const PASSWORD: &str = "correct horse battery staple 42";

/// A database with nested groups, a recycle bin, a note, an empty entry,
/// KeePassXC's and KeePass 2's one-time codes, history and an attachment.
fn database() -> Database {
    let mut db = Database::new();
    let mut root = db.root_mut();
    root.add_group()
        .edit(|g| g.name = "Work".into())
        .add_group()
        .edit(|g| g.name = "Banks".into())
        .add_entry()
        .edit(|e| {
            e.set_unprotected(fields::TITLE, "Bank");
            e.set_unprotected(fields::USERNAME, "ada");
            e.set_protected(fields::PASSWORD, "old-synthetic");
            let earlier: Entry = (**e).clone();
            e.set_protected(fields::PASSWORD, "hunter2-synthetic");
            e.history
                .get_or_insert_with(Default::default)
                .add_entry(earlier);
            e.set_unprotected(fields::URL, "https://bank.example");
            e.set_unprotected(fields::NOTES, "Synthetic note");
            e.set_unprotected("otp", "otpauth://totp/Bank:ada?secret=JBSWY3DPEHPK3PXP");
            e.set_unprotected("KP2A_URL_1", "https://app.bank.example");
            e.set_protected("PIN", "0000");
            e.set_unprotected("Member id", "42");
            e.tags.push("finance".into());
            e.add_attachment("scan.pdf", Value::unprotected(b"%PDF-synthetic".to_vec()));
        });
    root.add_entry().edit(|e| {
        e.set_unprotected(fields::TITLE, "Wifi");
        e.set_unprotected(fields::NOTES, "Code 1234");
    });
    root.add_entry().edit(|e| {
        e.set_unprotected(fields::TITLE, "Mail");
        e.set_unprotected(fields::USERNAME, "ada@example.com");
        e.set_protected(fields::PASSWORD, "mail-synthetic");
        e.set_unprotected("TimeOtp-Secret-Base32", "JBSW Y3DP EHPK 3PXP");
        e.set_unprotected("TimeOtp-Period", "60");
    });
    root.add_entry();
    let mut bin_group = root.add_group();
    bin_group.name = "Recycle Bin".into();
    let bin = bin_group.as_ref().id();
    bin_group.add_entry().edit(|e| {
        e.set_unprotected(fields::TITLE, "Old");
        e.set_protected(fields::PASSWORD, "gone");
    });
    db.meta.recyclebin_uuid = Some(bin.uuid());
    db
}

fn save(db: &Database, key: DatabaseKey) -> Vec<u8> {
    let mut out = Vec::new();
    db.save(&mut out, key).unwrap();
    out
}

fn options(password: Option<&str>, key_file: Option<&[u8]>) -> Options {
    Options {
        password: password.map(|p| Secret::new(p.to_owned())),
        key_file: key_file.map(|k| Secret::new(k.to_vec())),
        now_ms: NOW,
        ..Options::default()
    }
}

fn find<'a>(parsed: &'a Parsed, title: &str) -> &'a Item {
    &parsed
        .items
        .iter()
        .find(|(_, i)| i.title == title)
        .unwrap_or_else(|| panic!("no {title}"))
        .1
}

fn id_of(parsed: &Parsed, title: &str) -> ItemId {
    parsed
        .items
        .iter()
        .find(|(_, i)| i.title == title)
        .unwrap()
        .0
}

fn login(item: &Item) -> &pwm_vault::Login {
    match &item.kind {
        ItemKind::Login(l) => l,
        _ => panic!("not a login"),
    }
}

#[test]
fn a_kdbx4_database_maps_to_items() {
    let file = save(&database(), DatabaseKey::new().with_password(PASSWORD));
    assert_eq!(
        detect("vault.kdbx", &file),
        Some(Detected {
            format: Format::KeePass,
            needs_password: true
        })
    );
    let parsed = parse(Format::KeePass, &file, &options(Some(PASSWORD), None)).unwrap();
    let r = &parsed.report;
    assert_eq!((r.logins, r.notes, r.folders, r.trashed), (3, 1, 2, 1));
    assert_eq!(r.left_out.get(&LeftOut::Attachments), Some(&1));
    assert_eq!(r.left_out.get(&LeftOut::Empty), Some(&1));

    let bank = find(&parsed, "Bank");
    let l = login(bank);
    assert_eq!(l.username, "ada");
    assert_eq!(l.password, "hunter2-synthetic");
    let urls: Vec<&str> = l.urls.iter().map(|u| u.url.as_str()).collect();
    assert_eq!(urls, ["https://bank.example", "https://app.bank.example"]);
    assert!(l.totp.as_deref().unwrap().contains("JBSWY3DPEHPK3PXP"));
    assert_eq!(bank.notes, "Synthetic note");
    assert_eq!(bank.tags, ["finance"]);
    let field = |name: &str| bank.custom_fields.iter().find(|f| f.name == name).unwrap();
    assert_eq!(field("PIN").kind, FieldKind::Hidden, "protected");
    assert_eq!(field("Member id").kind, FieldKind::Text);
    assert_eq!(
        bank.custom_fields.len(),
        2,
        "otp and KP2A_URL are not fields"
    );
    let history: Vec<String> = bank
        .history()
        .iter()
        .map(|v| login(&v.item).password.clone())
        .collect();
    assert_eq!(history, ["old-synthetic"]);
    assert_eq!(bank.folder_id, Some(id_of(&parsed, "Banks")));
    let ItemKind::Folder(banks) = &find(&parsed, "Banks").kind else {
        panic!("not a folder")
    };
    assert_eq!(banks.parent_id, Some(id_of(&parsed, "Work")));

    assert!(matches!(find(&parsed, "Wifi").kind, ItemKind::Note));
    assert_eq!(find(&parsed, "Wifi").folder_id, None);
    let mail = login(find(&parsed, "Mail")).totp.clone().unwrap();
    assert!(
        mail.contains("secret=JBSWY3DPEHPK3PXP") && mail.contains("period=60"),
        "{mail}"
    );
    let old = find(&parsed, "Old");
    assert!(old.trashed_at.is_some());
    assert_eq!(old.folder_id, None, "the recycle bin is no folder");
    assert!(parsed.items.iter().all(|(_, i)| i.title != "Recycle Bin"));
}

#[test]
fn the_password_and_key_file_are_checked() {
    let file = save(&database(), DatabaseKey::new().with_password(PASSWORD));
    assert!(matches!(
        parse(Format::KeePass, &file, &options(None, None)),
        Err(ImportError::PasswordNeeded)
    ));
    assert!(matches!(
        parse(Format::KeePass, &file, &options(Some("not it"), None)),
        Err(ImportError::WrongPassword)
    ));

    let key_file = [7u8; 32];
    let keyed = save(
        &database(),
        DatabaseKey::new().with_keyfile(&mut &key_file[..]).unwrap(),
    );
    assert!(parse(Format::KeePass, &keyed, &options(None, Some(&key_file))).is_ok());
    assert!(matches!(
        parse(Format::KeePass, &keyed, &options(None, Some(&[8u8; 32]))),
        Err(ImportError::WrongPassword)
    ));
    let both = save(
        &database(),
        DatabaseKey::new()
            .with_password(PASSWORD)
            .with_keyfile(&mut &key_file[..])
            .unwrap(),
    );
    assert!(
        parse(
            Format::KeePass,
            &both,
            &options(Some(PASSWORD), Some(&key_file))
        )
        .is_ok()
    );
    assert!(matches!(
        parse(Format::KeePass, &both, &options(Some(PASSWORD), None)),
        Err(ImportError::WrongPassword)
    ));
}

#[test]
fn aes_kdf_databases_open_too() {
    let mut db = database();
    db.config.kdf_config = KdfConfig::Aes { rounds: 6_000 };
    let file = save(&db, DatabaseKey::new().with_password(PASSWORD));
    let parsed = parse(Format::KeePass, &file, &options(Some(PASSWORD), None)).unwrap();
    assert_eq!(parsed.report.logins, 3);
}

// --- The header check ---

/// A KDBX 4 header with these KDF parameters, and nothing after it.
fn kdbx4_header(kdf: &[(u8, &[u8], Vec<u8>)]) -> Vec<u8> {
    let mut dict = vec![0x00, 0x01];
    for (kind, key, value) in kdf {
        dict.push(*kind);
        dict.extend_from_slice(&(key.len() as i32).to_le_bytes());
        dict.extend_from_slice(key);
        dict.extend_from_slice(&(value.len() as i32).to_le_bytes());
        dict.extend_from_slice(value);
    }
    dict.push(0);
    let mut out = vec![
        0x03, 0xD9, 0xA2, 0x9A, 0x67, 0xFB, 0x4B, 0xB5, 0x01, 0x00, 0x04, 0x00,
    ];
    out.push(11);
    out.extend_from_slice(&(dict.len() as u32).to_le_bytes());
    out.extend_from_slice(&dict);
    out.extend_from_slice(&[0, 4, 0, 0, 0, 0x0D, 0x0A, 0x0D, 0x0A]);
    out
}

const ARGON2ID: [u8; 16] = [
    0x9E, 0x29, 0x8B, 0x19, 0x56, 0xDB, 0x47, 0x73, 0xB2, 0x3D, 0xFC, 0x3E, 0xC6, 0xF0, 0xA1, 0xE6,
];

fn code(bytes: &[u8]) -> String {
    parse(Format::KeePass, bytes, &options(Some(PASSWORD), None))
        .unwrap_err()
        .code()
        .to_owned()
}

#[test]
fn key_settings_a_device_cant_afford_are_refused_before_they_run() {
    let argon = |memory: u64, iterations: u64| {
        kdbx4_header(&[
            (0x42, b"$UUID", ARGON2ID.to_vec()),
            (0x05, b"M", memory.to_le_bytes().to_vec()),
            (0x05, b"I", iterations.to_le_bytes().to_vec()),
            (0x04, b"P", 2u32.to_le_bytes().to_vec()),
        ])
    };
    assert_eq!(
        code(&argon(2 * 1024 * 1024 * 1024, 2)),
        "import_keepass_too_costly"
    );
    assert_eq!(
        code(&argon(64 * 1024 * 1024, 1_000)),
        "import_keepass_too_costly"
    );
    // Within the bounds, the header passes and the (absent) rest is unreadable.
    assert_eq!(code(&argon(64 * 1024 * 1024, 2)), "import_unreadable");

    // KDBX 3.1: AES-KDF rounds are header field 6.
    let mut kdbx3 = vec![
        0x03, 0xD9, 0xA2, 0x9A, 0x67, 0xFB, 0x4B, 0xB5, 0x01, 0x00, 0x03, 0x00,
    ];
    kdbx3.push(6);
    kdbx3.extend_from_slice(&8u16.to_le_bytes());
    kdbx3.extend_from_slice(&500_000_000u64.to_le_bytes());
    kdbx3.extend_from_slice(&[0, 4, 0, 0x0D, 0x0A, 0x0D, 0x0A]);
    assert_eq!(code(&kdbx3), "import_keepass_too_costly");

    // KeePass 1 and unknown versions say so.
    assert_eq!(
        code(&[0x03, 0xD9, 0xA2, 0x9A, 0x65, 0xFB, 0x4B, 0xB5, 0, 0, 1, 0]),
        "import_keepass_kdb"
    );
    assert_eq!(
        code(&[0x03, 0xD9, 0xA2, 0x9A, 0x67, 0xFB, 0x4B, 0xB5, 0, 0, 5, 0]),
        "import_keepass_version"
    );
    // A truncated header never panics.
    let full = argon(64 * 1024 * 1024, 2);
    for len in 0..full.len() {
        let _ = parse(
            Format::KeePass,
            &full[..len],
            &options(Some(PASSWORD), None),
        );
    }
}

/// KDBX 3.1 files can't be written by the crate. Put small synthetic ones in
/// `fixtures/keepass/` (see its README) to run this; skipped without them.
#[test]
fn kdbx31_fixtures_open_when_present() {
    let dir = format!("{}/fixtures/keepass", env!("CARGO_MANIFEST_DIR"));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipped: no {dir}");
        return;
    };
    let mut seen = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("kdbx") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let parsed = parse(Format::KeePass, &bytes, &options(Some(PASSWORD), None))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(parsed.report.logins >= 1, "{}", path.display());
        seen += 1;
    }
    eprintln!("{seen} KDBX fixtures read");
}
