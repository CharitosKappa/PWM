//! Encrypted (`.pwmx`) and plaintext (JSON, CSV) exports (decision D24).

use std::sync::OnceLock;

use pwm_crypto::{CryptoError, Id, KdfParams, NewAccount, create_account};
use pwm_export::{
    CSV_COLUMNS, Export, ExportError, ExportGrant, ExportItem, ExportVault, GRANT_LIFETIME_MS,
    authorize_export, check_export_password, export_csv, export_json, export_pwmx, read_csv,
    read_json, read_pwmx,
};
use pwm_vault::{Folder, Item, ItemId, ItemKind, Login, LoginUrl, UrlMatch, VaultError};
use serde_json::Value;

const ACCOUNT: Id = [0xA1; 16];
const VAULT: Id = [0xB1; 16];
const MASTER: &str = "correct horse battery staple";
const EXPORT_PASSWORD: &str = "orbit-velvet-candle-harbor-ninth";
const NOW: i64 = 1_767_225_600_000;
/// The folder id the first fixture login points to.
const WORK_FOLDER: &str = "0f0e0d0c-0b0a-4908-8706-050403020100";
const FIXTURE_PASSWORD: &str = "fixture-orbit-velvet-candle-harbor";

fn account() -> &'static NewAccount {
    static ACCOUNT_KEYS: OnceLock<NewAccount> = OnceLock::new();
    ACCOUNT_KEYS.get_or_init(|| create_account(MASTER, &ACCOUNT, KdfParams::MOBILE_FLOOR).unwrap())
}

fn grant_with(password: &str, now: i64) -> Result<ExportGrant, ExportError> {
    let a = account();
    authorize_export(
        password,
        &a.secret_key,
        &ACCOUNT,
        &a.salt,
        &a.params,
        &a.sealed_keyset,
        now,
    )
}

fn grant() -> ExportGrant {
    grant_with(MASTER, NOW).unwrap()
}

fn fixture_items() -> Vec<ExportItem> {
    let raw: Vec<Value> =
        serde_json::from_str(include_str!("../../pwm-vault/tests/fixtures/items_v1.json")).unwrap();
    raw.iter()
        .enumerate()
        .map(|(n, v)| ExportItem {
            id: if n == 4 {
                WORK_FOLDER.parse().unwrap()
            } else {
                ItemId::from_bytes([n as u8 + 1; 16])
            },
            item: Item::from_json(&v.to_string()).unwrap(),
        })
        .collect()
}

/// A nested folder and a login whose values need CSV quoting.
fn tricky_items() -> Vec<ExportItem> {
    let mut folder = Folder::default();
    folder.parent_id = Some(WORK_FOLDER.parse().unwrap());
    let email_folder = ItemId::from_bytes([0xE0; 16]);
    let mut login = Login::default();
    login.username = "o'brien, \"bob\"".into();
    login.password = "=SUM(A1)+\"x\",\n-@".into();
    let mut url = LoginUrl::default();
    url.url = "https://mail.example".into();
    let mut url2 = LoginUrl::default();
    url2.url = "https://mail.example/inbox".into();
    url2.r#match = UrlMatch::Exact;
    login.urls = vec![url, url2];
    login.totp = Some("otpauth://totp/Mail?secret=GEZDGNBVGY3TQOJQ".into());
    let mut item = Item::new(ItemKind::Login(login), "Mail ☕ \"quoted\"", NOW);
    item.notes = "line one\r\nline two, with comma".into();
    item.tags = vec!["a,b".into(), "c".into()];
    item.folder_id = Some(email_folder);
    vec![
        ExportItem {
            id: email_folder,
            item: Item::new(ItemKind::Folder(folder), "Email", NOW),
        },
        ExportItem {
            id: ItemId::from_bytes([0xE1; 16]),
            item,
        },
    ]
}

fn vaults() -> Vec<ExportVault> {
    let mut items = fixture_items();
    items.extend(tricky_items());
    vec![
        ExportVault { id: VAULT, items },
        ExportVault {
            id: [0xB2; 16],
            items: Vec::new(),
        },
    ]
}

// --- Re-authentication ---

#[test]
fn every_export_needs_the_master_password() {
    assert!(matches!(
        grant_with("wrong horse battery staple", NOW),
        Err(ExportError::Crypto(CryptoError::DecryptionFailed))
    ));
    assert!(grant_with(MASTER, NOW).is_ok());
}

#[test]
fn a_grant_expires_after_five_minutes() {
    assert_eq!(GRANT_LIFETIME_MS, 5 * 60 * 1000);
    assert!(export_json(grant(), NOW + GRANT_LIFETIME_MS, &vaults()).is_ok());
    assert!(matches!(
        export_json(grant(), NOW + GRANT_LIFETIME_MS + 1, &vaults()),
        Err(ExportError::GrantExpired)
    ));
    // A clock that moved backwards does not extend it.
    assert!(matches!(
        export_csv(grant(), NOW - 1, &vaults()),
        Err(ExportError::GrantExpired)
    ));
}

// --- JSON ---

#[test]
fn json_round_trip_keeps_every_item_unchanged() {
    let json = export_json(grant(), NOW, &vaults()).unwrap();
    let back = read_json(json.expose_secret()).unwrap();
    assert_eq!(
        back,
        Export {
            exported_at: NOW,
            vaults: vaults()
        }
    );
}

#[test]
fn json_layout_is_the_item_json_plus_id() {
    let json = export_json(grant(), NOW, &vaults()).unwrap();
    let doc: Value = serde_json::from_slice(json.expose_secret()).unwrap();
    assert_eq!(doc["format"], "pwm-export");
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["exported_at"], NOW);
    assert_eq!(
        doc["vaults"][0]["id"],
        "b1b1b1b1-b1b1-b1b1-b1b1-b1b1b1b1b1b1"
    );
    assert_eq!(doc["vaults"][1]["items"], Value::Array(Vec::new()));
    let raw: Vec<Value> =
        serde_json::from_str(include_str!("../../pwm-vault/tests/fixtures/items_v1.json")).unwrap();
    for (n, original) in raw.iter().enumerate() {
        let mut exported = doc["vaults"][0]["items"][n].clone();
        let id = exported.as_object_mut().unwrap().remove("id").unwrap();
        assert_eq!(id, fixture_items()[n].id.to_string());
        let reparsed = Item::from_json(&exported.to_string()).unwrap();
        assert!(
            reparsed == Item::from_json(&original.to_string()).unwrap(),
            "item {n}"
        );
    }
}

#[test]
fn json_reader_rejects_other_documents() {
    for doc in [
        &b"not json"[..],
        b"{}",
        b"[]",
        br#"{"format":"other","version":1}"#,
    ] {
        assert!(
            matches!(read_json(doc), Err(ExportError::NotAnExport)),
            "{doc:?}"
        );
    }
    assert!(matches!(
        read_json(br#"{"format":"pwm-export","version":2,"exported_at":0,"vaults":[]}"#),
        Err(ExportError::UnsupportedVersion(2))
    ));
    assert!(matches!(
        read_json(br#"{"format":"pwm-export","version":1,"exported_at":0,"vaults":[{"id":"x","items":[]}]}"#),
        Err(ExportError::NotAnExport)
    ));
    let no_id = br#"{"format":"pwm-export","version":1,"exported_at":0,"vaults":[{"id":"b1b1b1b1-b1b1-b1b1-b1b1-b1b1b1b1b1b1","items":[{"v":1,"type":"note","title":"t","created_at":0,"updated_at":0}]}]}"#;
    assert!(matches!(
        read_json(no_id),
        Err(ExportError::Vault(VaultError::InvalidItem))
    ));
}

// --- .pwmx ---

#[test]
fn pwmx_round_trip_and_padding() {
    let file = export_pwmx(grant(), NOW, &vaults(), EXPORT_PASSWORD).unwrap();
    assert_eq!(&file[..4], b"PWMX");
    // Header 33 bytes, envelope overhead 41 bytes, payload in 4 KiB steps.
    assert_eq!((file.len() - 33 - 41) % 4096, 0);
    let back = read_pwmx(&file, EXPORT_PASSWORD).unwrap();
    assert_eq!(back.vaults, vaults());
    assert_eq!(back.exported_at, NOW);
    assert!(matches!(
        read_pwmx(&file, "orbit-velvet-candle-harbor-tenth"),
        Err(ExportError::Crypto(CryptoError::DecryptionFailed))
    ));
}

#[test]
fn weak_export_passwords_are_refused() {
    for weak in ["password1", "aaaaaaaaaaaa", "qwerty123"] {
        assert!(matches!(
            check_export_password(weak),
            Err(ExportError::WeakPassword)
        ));
        assert!(matches!(
            export_pwmx(grant(), NOW, &vaults(), weak),
            Err(ExportError::WeakPassword)
        ));
    }
    assert!(check_export_password(EXPORT_PASSWORD).is_ok());
}

#[test]
fn frozen_v1_fixtures_still_open() {
    let pwmx = include_bytes!("fixtures/export_v1.pwmx");
    let json = include_bytes!("fixtures/export_v1.json");
    let expected = vec![ExportVault {
        id: VAULT,
        items: fixture_items(),
    }];
    assert_eq!(read_pwmx(pwmx, FIXTURE_PASSWORD).unwrap().vaults, expected);
    assert_eq!(read_json(json).unwrap().vaults, expected);
}

/// Writes the frozen fixtures once. Run with `--ignored` only when adding a
/// new format version; existing non-empty files are never overwritten.
#[test]
#[ignore]
fn write_v1_fixtures() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let vaults = vec![ExportVault {
        id: VAULT,
        items: fixture_items(),
    }];
    let pwmx = dir.join("export_v1.pwmx");
    if is_missing(&pwmx) {
        let file = export_pwmx(grant(), NOW, &vaults, FIXTURE_PASSWORD).unwrap();
        std::fs::write(pwmx, file).unwrap();
    }
    let json = dir.join("export_v1.json");
    if is_missing(&json) {
        let doc = export_json(grant(), NOW, &vaults).unwrap();
        std::fs::write(json, doc.expose_secret()).unwrap();
    }
}

fn is_missing(path: &std::path::Path) -> bool {
    std::fs::metadata(path).map_or(true, |m| m.len() == 0)
}

// --- CSV ---

fn rows(csv: &[u8]) -> Vec<Vec<String>> {
    // Test-only splitter for the exact output shape: every field quoted, CRLF rows.
    let text = std::str::from_utf8(csv).unwrap();
    let mut rows = Vec::new();
    let (mut row, mut field, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (false, '"') => quoted = true,
            (true, '"') if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => panic!("unquoted character {c:?}"),
        }
    }
    assert!(row.is_empty() && field.is_empty(), "missing final CRLF");
    rows
}

fn cell<'a>(row: &'a [String], column: &str) -> &'a str {
    &row[CSV_COLUMNS.iter().position(|c| *c == column).unwrap()]
}

#[test]
fn csv_has_one_row_per_active_item_of_a_known_type() {
    let csv = export_csv(grant(), NOW, &vaults()).unwrap();
    let rows = rows(csv.expose_secret());
    assert_eq!(rows[0], CSV_COLUMNS);
    let titles: Vec<&str> = rows[1..].iter().map(|r| cell(r, "title")).collect();
    // No folders, no unknown types, nothing from the trash.
    assert_eq!(
        titles,
        [
            "Example",
            "Wi-Fi codes",
            "Visa",
            "Home",
            "Fields from a newer app version",
            "Item from schema v2",
            "Mail ☕ \"quoted\""
        ]
    );
    let mail = &rows[7];
    assert_eq!(cell(mail, "type"), "login");
    assert_eq!(cell(mail, "folder"), "Work/Email");
    assert_eq!(cell(mail, "tags"), "a,b\nc");
    assert_eq!(
        cell(mail, "url"),
        "https://mail.example\nhttps://mail.example/inbox"
    );
    // Formula characters are not escaped: the password stays exactly as stored.
    assert_eq!(cell(mail, "password"), "=SUM(A1)+\"x\",\n-@");
    assert_eq!(cell(mail, "username"), "o'brien, \"bob\"");
    let card = &rows[3];
    assert_eq!(cell(card, "card_number"), "4111 1111 1111 1111");
    assert_eq!(cell(card, "card_expiry_month"), "12");
    assert_eq!(cell(card, "card_expiry_year"), "2030");
    assert_eq!(cell(card, "password"), "");
    let identity = &rows[4];
    assert_eq!(
        cell(identity, "identity_address_lines"),
        "1 Main Street\nFlat 2"
    );
    assert_eq!(cell(&rows[1], "favorite"), "true");
    assert_eq!(cell(&rows[2], "favorite"), "false");
}

/// What CSV leaves out (D24): history, trash, passkeys, custom fields, ids,
/// timestamps, URL match modes and unknown fields.
fn csv_view(item: &Item) -> Item {
    let mut view = Item::new(item.kind.clone(), item.title.clone(), NOW);
    view.notes = item.notes.clone();
    view.favorite = item.favorite;
    view.tags = item.tags.clone();
    view.kind = match &item.kind {
        ItemKind::Login(login) => {
            let mut l = Login::default();
            l.username = login.username.clone();
            l.password = login.password.clone();
            l.totp = login.totp.clone();
            l.urls = login
                .urls
                .iter()
                .map(|u| {
                    let mut url = LoginUrl::default();
                    url.url = u.url.clone();
                    url
                })
                .collect();
            ItemKind::Login(l)
        }
        other => strip_extra(other),
    };
    view
}

/// Card and identity fields without unknown fields, via a JSON round trip.
fn strip_extra(kind: &ItemKind) -> ItemKind {
    let item = Item::new(kind.clone(), "", NOW);
    let mut value: Value = serde_json::from_str(item.to_json().unwrap().expose_secret()).unwrap();
    let known = [
        "cardholder",
        "number",
        "expiry_month",
        "expiry_year",
        "security_code",
        "brand",
        "first_name",
        "middle_name",
        "last_name",
        "email",
        "phone",
        "company",
        "address_lines",
        "city",
        "region",
        "postal_code",
        "country",
    ];
    for payload in ["card", "identity"] {
        if let Some(map) = value[payload].as_object_mut() {
            map.retain(|k, _| known.contains(&k.as_str()));
        }
    }
    Item::from_json(&value.to_string()).unwrap().kind.clone()
}

fn folder_path(items: &[ExportItem], id: Option<ItemId>) -> String {
    let mut parts = Vec::new();
    let mut next = id;
    while let Some(id) = next {
        let folder = items.iter().find(|i| i.id == id).unwrap();
        parts.insert(0, folder.item.title.clone());
        next = match &folder.item.kind {
            ItemKind::Folder(f) => f.parent_id,
            _ => panic!("not a folder"),
        };
    }
    parts.join("/")
}

#[test]
fn csv_round_trip_keeps_what_csv_can_hold() {
    let all = vaults();
    let csv = export_csv(grant(), NOW, &all).unwrap();
    let imported = read_csv(csv.expose_secret(), NOW).unwrap();
    let original = &all[0].items;
    let expected: Vec<&ExportItem> = original
        .iter()
        .filter(|i| {
            !i.item.is_trashed()
                && matches!(
                    i.item.kind,
                    ItemKind::Login(_) | ItemKind::Note | ItemKind::Card(_) | ItemKind::Identity(_)
                )
        })
        .collect();
    let got: Vec<&ExportItem> = imported
        .iter()
        .filter(|i| !matches!(i.item.kind, ItemKind::Folder(_)))
        .collect();
    assert_eq!(got.len(), expected.len());
    for (e, g) in expected.iter().zip(&got) {
        assert!(csv_view(&e.item) == csv_view(&g.item), "{}", e.item.title);
        assert_eq!(
            folder_path(original, e.item.folder_id),
            folder_path(&imported, g.item.folder_id)
        );
        assert_eq!(g.item.created_at, NOW);
        assert!(g.item.history().is_empty());
    }
    // Folders are recreated once per path.
    let folders: Vec<String> = imported
        .iter()
        .filter(|i| matches!(i.item.kind, ItemKind::Folder(_)))
        .map(|i| folder_path(&imported, Some(i.id)))
        .collect();
    assert_eq!(folders, ["Work", "Work/Email"]);
}

#[test]
fn csv_reader_rejects_bad_input_without_quoting_it() {
    let header = CSV_COLUMNS.join(",");
    let row = |cells: &[(&str, &str)]| {
        let mut values = vec![String::new(); CSV_COLUMNS.len()];
        for (column, value) in cells {
            values[CSV_COLUMNS.iter().position(|c| c == column).unwrap()] = (*value).into();
        }
        values.join(",")
    };
    assert!(matches!(
        read_csv(b"a,b\r\n", NOW),
        Err(ExportError::NotAnExport)
    ));
    assert!(matches!(read_csv(b"", NOW), Err(ExportError::NotAnExport)));
    let cases = [
        (format!("{header}\r\nlogin,short\r\n"), "column count"),
        (
            format!("{header}\r\n{}\r\n", row(&[("type", "secretwifi")])),
            "type",
        ),
        (
            format!(
                "{header}\r\n{}\r\n",
                row(&[("type", "note"), ("favorite", "secretyes")])
            ),
            "favorite",
        ),
        (
            format!(
                "{header}\r\n{}\r\n",
                row(&[("type", "card"), ("card_expiry_month", "13secret")])
            ),
            "month",
        ),
    ];
    for (csv, what) in cases {
        let err = read_csv(csv.as_bytes(), NOW).unwrap_err();
        assert!(
            matches!(err, ExportError::InvalidCsv { row: 2, .. }),
            "{what}: {err}"
        );
        assert!(
            !format!("{err} {err:?}").contains("secret"),
            "{what}: {err}"
        );
    }
    // A valid file without a final line break is fine.
    let ok = format!("{header}\r\n{}", row(&[("type", "note"), ("title", "t")]));
    assert_eq!(read_csv(ok.as_bytes(), NOW).unwrap().len(), 1);
}
