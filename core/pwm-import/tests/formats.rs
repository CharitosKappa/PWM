//! Every format on synthetic fixtures (D51 §10): what maps where, what is
//! left out, the encrypted exports, our own exports into any account, and
//! the limits against hostile files.

use std::io::Write as _;

use pwm_crypto::Secret;
use pwm_import::{
    ColumnMapping, Detected, Format, ImportError, LeftOut, MAX_FILE_BYTES, Options, Parsed,
    csv_columns, detect, parse,
};
use pwm_vault::{FieldKind, Item, ItemId, ItemKind, UrlMatch};

const NOW: i64 = 1_767_225_600_000;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn zip_of(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}

fn options() -> Options {
    Options {
        now_ms: NOW,
        ..Options::default()
    }
}

fn with_password(password: &str) -> Options {
    Options {
        password: Some(Secret::new(password.to_owned())),
        ..options()
    }
}

fn run(format: Format, bytes: &[u8]) -> Parsed {
    parse(format, bytes, &options()).unwrap()
}

fn find<'a>(parsed: &'a Parsed, title: &str) -> &'a Item {
    &parsed
        .items
        .iter()
        .find(|(_, i)| i.title == title)
        .unwrap_or_else(|| panic!("no item {title}"))
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

fn card(item: &Item) -> &pwm_vault::Card {
    match &item.kind {
        ItemKind::Card(c) => c,
        _ => panic!("not a card"),
    }
}

fn identity(item: &Item) -> &pwm_vault::Identity {
    match &item.kind {
        ItemKind::Identity(i) => i,
        _ => panic!("not an identity"),
    }
}

fn parent(parsed: &Parsed, folder: &str) -> Option<ItemId> {
    match &find(parsed, folder).kind {
        ItemKind::Folder(f) => f.parent_id,
        _ => panic!("not a folder"),
    }
}

fn field<'a>(item: &'a Item, name: &str) -> &'a pwm_vault::CustomField {
    item.custom_fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn left(parsed: &Parsed, reason: LeftOut) -> usize {
    parsed.report.left_out.get(&reason).copied().unwrap_or(0)
}

/// Folders come before the items that use them.
fn folders_first(parsed: &Parsed) {
    let first_item = parsed
        .items
        .iter()
        .position(|(_, i)| !matches!(i.kind, ItemKind::Folder(_)))
        .unwrap_or(parsed.items.len());
    assert!(
        parsed.items[first_item..]
            .iter()
            .all(|(_, i)| !matches!(i.kind, ItemKind::Folder(_)))
    );
}

// --- CSV formats ---

#[test]
fn lastpass() {
    let parsed = run(Format::LastPass, &fixture("lastpass.csv"));
    folders_first(&parsed);
    let r = &parsed.report;
    assert_eq!(
        (r.logins, r.cards, r.identities, r.notes, r.folders),
        (2, 1, 1, 1, 2)
    );
    let bank = find(&parsed, "Bank");
    assert_eq!(login(bank).username, "ada");
    assert_eq!(login(bank).urls[0].url, "https://bank.example");
    assert!(
        login(bank)
            .totp
            .as_deref()
            .unwrap()
            .starts_with("otpauth://totp/")
    );
    assert!(bank.favorite);
    assert_eq!(bank.notes, "Account notes");
    assert_eq!(bank.folder_id, Some(id_of(&parsed, "Banks")));
    assert_eq!(parent(&parsed, "Banks"), Some(id_of(&parsed, "Finance")));

    let visa = find(&parsed, "Visa card");
    assert_eq!(card(visa).cardholder, "Ada Lovelace");
    assert_eq!(card(visa).number, "4111111111111111");
    assert_eq!(
        (card(visa).expiry_month, card(visa).expiry_year),
        (Some(1), Some(2030))
    );
    assert_eq!(visa.notes, "first line\nsecond line");
    assert_eq!(visa.folder_id, Some(id_of(&parsed, "Finance")));
    let home = find(&parsed, "Home address");
    assert_eq!(identity(home).address_lines, ["1 Example Street", "Flat 2"]);
    assert_eq!(identity(home).postal_code, "N1 1AA");
    assert_eq!(field(home, "Title").value, "Ms");
    assert_eq!(find(&parsed, "Plain note").notes, "Wifi code 1234");
}

#[test]
fn nordpass() {
    let parsed = run(Format::NordPass, &fixture("nordpass.csv"));
    let r = &parsed.report;
    assert_eq!(
        (r.logins, r.cards, r.identities, r.notes, r.folders),
        (1, 1, 1, 1, 1)
    );
    assert_eq!(left(&parsed, LeftOut::UnknownTypes), 1);
    let bank = find(&parsed, "Bank");
    let urls: Vec<&str> = login(bank).urls.iter().map(|u| u.url.as_str()).collect();
    assert_eq!(urls, ["https://bank.example", "https://app.bank.example"]);
    assert_eq!(field(bank, "Security answer").kind, FieldKind::Hidden);
    assert!(login(bank).totp.is_some());
    assert_eq!(bank.folder_id, Some(id_of(&parsed, "Work")));
    let visa = find(&parsed, "Visa");
    assert_eq!(
        (card(visa).expiry_month, card(visa).expiry_year),
        (Some(5), Some(2030))
    );
    assert_eq!(field(visa, "PIN").kind, FieldKind::Hidden);
    let me = find(&parsed, "Me");
    assert_eq!(
        (
            identity(me).first_name.as_str(),
            identity(me).last_name.as_str()
        ),
        ("Ada", "Lovelace")
    );
}

#[test]
fn a_browser_csv_needs_no_mapping() {
    let bytes = fixture("chrome.csv");
    let columns = csv_columns(&bytes).unwrap();
    assert_eq!(
        columns.headers,
        ["name", "url", "username", "password", "note"]
    );
    assert_eq!(columns.rows, 2, "the empty row doesn't count");
    let parsed = run(Format::Csv, &bytes);
    assert_eq!(parsed.report.logins, 2);
    assert_eq!(
        login(find(&parsed, "mail.example")).password,
        "mail-pass-synthetic"
    );
    assert_eq!(find(&parsed, "mail.example").notes, "Synthetic note");
}

#[test]
fn a_csv_mapping_chosen_by_the_user() {
    let bytes = b"Site,Who,Secret,Group\nBank,ada,pw,Work/Banks\n,,,\nMail,,,\n".to_vec();
    let mapping = ColumnMapping {
        title: Some(0),
        username: Some(1),
        password: Some(2),
        folder: Some(3),
        ..ColumnMapping::default()
    };
    let parsed = parse(
        Format::Csv,
        &bytes,
        &Options {
            mapping: Some(mapping.clone()),
            ..options()
        },
    )
    .unwrap();
    assert_eq!(parsed.report.logins, 2);
    assert_eq!(parsed.report.folders, 2);
    assert_eq!(login(find(&parsed, "Bank")).password, "pw");
    // A column past the header is refused, never read out of bounds.
    let bad = ColumnMapping {
        notes: Some(9),
        ..mapping
    };
    let refused = parse(
        Format::Csv,
        &bytes,
        &Options {
            mapping: Some(bad),
            ..options()
        },
    );
    assert!(matches!(refused, Err(ImportError::Unreadable)));
}

#[test]
fn dashlane_csv_zip_lone_csv_and_json() {
    let zip = zip_of(&[
        ("credentials.csv", fixture("dashlane/credentials.csv")),
        ("securenotes.csv", fixture("dashlane/securenotes.csv")),
        ("payments.csv", fixture("dashlane/payments.csv")),
    ]);
    let parsed = run(Format::Dashlane, &zip);
    let r = &parsed.report;
    assert_eq!((r.logins, r.notes, r.cards, r.folders), (2, 2, 1, 1));
    let bank = find(&parsed, "Bank");
    assert_eq!(field(bank, "Second username").value, "ada.l");
    assert!(login(bank).totp.is_some());
    assert_eq!(bank.folder_id, Some(id_of(&parsed, "Finance")));
    assert_eq!(find(&parsed, "Wifi").notes, "Code 1234\nsecond line");
    assert_eq!(
        (
            card(find(&parsed, "Visa")).expiry_month,
            card(find(&parsed, "Visa")).expiry_year
        ),
        (Some(5), Some(2030))
    );
    assert_eq!(
        field(find(&parsed, "Savings"), "Account number").kind,
        FieldKind::Hidden
    );

    let lone = run(Format::Dashlane, &fixture("dashlane/credentials.csv"));
    assert_eq!(lone.report.logins, 2);

    let json = run(Format::Dashlane, &fixture("dashlane.json"));
    let r = &json.report;
    assert_eq!((r.logins, r.notes, r.cards), (1, 1, 1));
    assert_eq!(left(&json, LeftOut::UnknownTypes), 1);
    assert_eq!(login(find(&json, "Bank")).urls[0].url, "bank.example");
}

// --- JSON formats ---

#[test]
fn bitwarden() {
    let parsed = run(Format::Bitwarden, &fixture("bitwarden.json"));
    folders_first(&parsed);
    let r = &parsed.report;
    assert_eq!(
        (
            r.logins,
            r.notes,
            r.cards,
            r.identities,
            r.folders,
            r.trashed
        ),
        (3, 1, 1, 1, 2, 1)
    );
    assert_eq!(left(&parsed, LeftOut::SshKeys), 1);
    assert_eq!(left(&parsed, LeftOut::Passkeys), 1, "the Ed25519 one");
    let bank = find(&parsed, "Bank");
    let l = login(bank);
    let modes: Vec<UrlMatch> = l.urls.iter().map(|u| u.r#match.clone()).collect();
    assert_eq!(modes, [UrlMatch::Domain, UrlMatch::Host, UrlMatch::Regex]);
    assert!(l.totp.as_deref().unwrap().contains("JBSWY3DPEHPK3PXP"));
    assert_eq!(l.passkeys.len(), 1);
    let passkey = &l.passkeys[0];
    assert_eq!(passkey.rp_id, "bank.example");
    assert_eq!(
        passkey.credential_id,
        (0..16u8).map(|i| i * 0x11).collect::<Vec<_>>()
    );
    assert_eq!(passkey.user_handle, b"user-1");
    assert_eq!(passkey.sign_count, 3);
    assert!(bank.favorite);
    assert_eq!(bank.created_at, 1_704_164_645_678);
    assert_eq!(bank.folder_id, Some(id_of(&parsed, "Banks")));
    assert_eq!(parent(&parsed, "Banks"), Some(id_of(&parsed, "Work")));
    // Password history, newest first.
    let history: Vec<String> = bank
        .history()
        .iter()
        .map(|v| login(&v.item).password.clone())
        .collect();
    assert_eq!(history, ["old-synthetic", "older-synthetic"]);
    // Custom fields: the linked one is skipped.
    assert_eq!(bank.custom_fields.len(), 2);
    assert_eq!(field(bank, "Security answer").kind, FieldKind::Hidden);

    let visa = find(&parsed, "Visa");
    assert_eq!(
        (card(visa).expiry_month, card(visa).expiry_year),
        (Some(5), Some(2030))
    );
    let me = find(&parsed, "Me");
    assert_eq!(identity(me).company, "Analytical Engines");
    assert_eq!(field(me, "Social security number").kind, FieldKind::Hidden);
    // An empty name takes the URL's host.
    assert_eq!(login(find(&parsed, "nameless.example")).username, "x");
    assert!(find(&parsed, "Old").trashed_at.is_some());
}

#[test]
fn bitwarden_password_protected() {
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/bitwarden_export.json")).unwrap();
    let v = &doc["vectors"][0];
    let file = serde_json::to_vec(&v["export"]).unwrap();
    let password = v["password"].as_str().unwrap();

    let detected = detect("export.json", &file).unwrap();
    assert_eq!(
        detected,
        Detected {
            format: Format::Bitwarden,
            needs_password: true
        }
    );
    let parsed = parse(Format::Bitwarden, &file, &with_password(password)).unwrap();
    let bank = find(&parsed, "Bank");
    assert_eq!(login(bank).password, "hunter2-synthetic");
    assert_eq!(login(bank).urls[0].r#match, UrlMatch::Host);
    assert_eq!(parsed.report.folders, 2);

    assert!(matches!(
        parse(Format::Bitwarden, &file, &options()),
        Err(ImportError::PasswordNeeded)
    ));
    assert!(matches!(
        parse(Format::Bitwarden, &file, &with_password("wrong password")),
        Err(ImportError::WrongPassword)
    ));
    // A KDF a device can't afford is refused before it runs.
    let mut costly = v["export"].clone();
    costly["kdfIterations"] = 100_000_000.into();
    let costly = serde_json::to_vec(&costly).unwrap();
    assert!(matches!(
        parse(Format::Bitwarden, &costly, &with_password(password)),
        Err(ImportError::Unreadable)
    ));
}

#[test]
fn bitwarden_argon2id() {
    use aes::Aes256;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use cbc::cipher::block_padding::Pkcs7;
    use cbc::cipher::{BlockModeEncrypt, KeyIvInit};
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::{Digest, Sha256};

    let (password, salt) = ("correct horse battery staple 42", "argon-salt");
    let params = argon2::Params::new(16 * 1024, 2, 1, Some(32)).unwrap();
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(
            password.as_bytes(),
            &Sha256::digest(salt.as_bytes()),
            &mut key,
        )
        .unwrap();
    let hkdf = hkdf::Hkdf::<Sha256>::from_prk(&key).unwrap();
    let (mut enc, mut mac) = ([0u8; 32], [0u8; 32]);
    hkdf.expand(b"enc", &mut enc).unwrap();
    hkdf.expand(b"mac", &mut mac).unwrap();
    let seal = |plain: &[u8]| {
        let iv = [7u8; 16];
        let mut buf = plain.to_vec();
        buf.resize(plain.len() + 16, 0);
        let ct = cbc::Encryptor::<Aes256>::new_from_slices(&enc, &iv)
            .unwrap()
            .encrypt_padded::<Pkcs7>(&mut buf, plain.len())
            .unwrap()
            .to_vec();
        let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(&mac).unwrap();
        m.update(&iv);
        m.update(&ct);
        let tag = m.finalize().into_bytes();
        format!(
            "2.{}|{}|{}",
            STANDARD.encode(iv),
            STANDARD.encode(&ct),
            STANDARD.encode(tag)
        )
    };
    let inner = br#"{"encrypted":false,"items":[{"type":2,"name":"Wifi","notes":"Code 1234"}]}"#;
    let file = serde_json::json!({
        "encrypted": true, "passwordProtected": true, "salt": salt,
        "kdfType": 1, "kdfIterations": 2, "kdfMemory": 16, "kdfParallelism": 1,
        "encKeyValidation_DO_NOT_EDIT": seal(b"check"), "data": seal(inner),
    });
    let file = serde_json::to_vec(&file).unwrap();
    let parsed = parse(Format::Bitwarden, &file, &with_password(password)).unwrap();
    assert_eq!(find(&parsed, "Wifi").notes, "Code 1234");
}

#[test]
fn bitwarden_account_restricted_is_refused_with_a_reason() {
    let file = br#"{"encrypted":true,"encKeyValidation_DO_NOT_EDIT":"2.x|y|z","data":"2.x|y|z","items":[]}"#;
    let err = parse(Format::Bitwarden, file, &options()).unwrap_err();
    assert_eq!(err.code(), "import_bitwarden_account_restricted");
}

#[test]
fn onepassword() {
    let zip = zip_of(&[
        ("export.data", fixture("1password/export.data")),
        ("files/scan.pdf", b"%PDF".to_vec()),
    ]);
    assert_eq!(
        detect("export.1pux", &zip).map(|d| d.format),
        Some(Format::OnePassword)
    );
    let parsed = run(Format::OnePassword, &zip);
    let r = &parsed.report;
    assert_eq!(
        (
            r.logins,
            r.cards,
            r.notes,
            r.identities,
            r.folders,
            r.trashed
        ),
        (1, 1, 2, 1, 0, 1)
    );
    assert_eq!(
        left(&parsed, LeftOut::Attachments),
        2,
        "the document and the file field"
    );
    let bank = find(&parsed, "Bank");
    assert_eq!(login(bank).username, "ada");
    assert_eq!(login(bank).password, "hunter2-synthetic");
    assert!(login(bank).totp.is_some());
    assert_eq!(field(bank, "Security answer").kind, FieldKind::Hidden);
    assert_eq!(field(bank, "Member since").kind, FieldKind::Date);
    assert_eq!(bank.tags, ["finance"]);
    assert!(bank.favorite);
    assert_eq!(bank.created_at, 1_704_164_645_000);
    assert_eq!(bank.history().len(), 1);
    let visa = find(&parsed, "Visa");
    assert_eq!(card(visa).number, "4111111111111111");
    assert_eq!(card(visa).security_code, "123");
    assert_eq!(card(visa).brand, "visa");
    assert_eq!(
        (card(visa).expiry_month, card(visa).expiry_year),
        (Some(5), Some(2030))
    );
    assert_eq!(field(visa, "PIN").kind, FieldKind::Hidden);
    assert!(find(&parsed, "Wifi").trashed_at.is_some(), "archived");
    let me = find(&parsed, "Me");
    assert_eq!(identity(me).email, "ada@example.com");
    assert_eq!(identity(me).city, "London");
    assert_eq!(
        field(find(&parsed, "Savings"), "account number").value,
        "12345678"
    );
}

#[test]
fn keeper() {
    let parsed = run(Format::Keeper, &fixture("keeper.json"));
    let r = &parsed.report;
    assert_eq!((r.logins, r.cards, r.identities, r.notes), (1, 1, 1, 1));
    assert_eq!(left(&parsed, LeftOut::Attachments), 1);
    let bank = find(&parsed, "Bank");
    assert!(login(bank).totp.is_some());
    assert_eq!(field(bank, "Member id").value, "42");
    assert_eq!(field(bank, "Security answer").kind, FieldKind::Hidden);
    assert_eq!(field(bank, "PIN code").kind, FieldKind::Hidden);
    assert_eq!(bank.folder_id, Some(id_of(&parsed, "Banks")));
    assert_eq!(parent(&parsed, "Banks"), Some(id_of(&parsed, "Personal")));
    let visa = find(&parsed, "Visa");
    assert_eq!(card(visa).cardholder, "Ada Lovelace");
    assert_eq!(
        (card(visa).expiry_month, card(visa).expiry_year),
        (Some(5), Some(2030))
    );
    assert!(visa.custom_fields.is_empty(), "every card field mapped");
    assert_eq!(visa.folder_id, Some(id_of(&parsed, "Cards")));
    assert_eq!(parent(&parsed, "Cards"), Some(id_of(&parsed, "Team")));
    assert_eq!(
        identity(find(&parsed, "Home")).address_lines,
        ["1 Example Street"]
    );
}

#[test]
fn proton_pass() {
    let zip = zip_of(&[("Proton Pass/data.json", fixture("proton/data.json"))]);
    assert_eq!(
        detect("export.zip", &zip).map(|d| d.format),
        Some(Format::ProtonPass)
    );
    let parsed = run(Format::ProtonPass, &zip);
    let r = &parsed.report;
    assert_eq!(
        (
            r.logins,
            r.cards,
            r.notes,
            r.identities,
            r.folders,
            r.trashed
        ),
        (1, 1, 1, 1, 2, 1)
    );
    assert_eq!(left(&parsed, LeftOut::Aliases), 1);
    assert_eq!(left(&parsed, LeftOut::Passkeys), 1);
    let bank = find(&parsed, "Bank");
    assert_eq!(login(bank).username, "ada");
    assert_eq!(field(bank, "Email").value, "ada@example.com");
    assert_eq!(login(bank).urls.len(), 2);
    assert!(bank.favorite);
    assert_eq!(bank.folder_id, Some(id_of(&parsed, "Personal")));
    assert_eq!(
        (
            card(find(&parsed, "Visa")).expiry_month,
            card(find(&parsed, "Visa")).expiry_year
        ),
        (Some(5), Some(2030))
    );
    assert_eq!(identity(find(&parsed, "Me")).first_name, "Ada Lovelace");
    // The data.json on its own reads the same.
    assert_eq!(
        run(Format::ProtonPass, &fixture("proton/data.json")).report,
        parsed.report
    );
    // The PGP-encrypted export is refused with a reason.
    let pgp = zip_of(&[(
        "Proton Pass/data.pgp",
        b"-----BEGIN PGP MESSAGE-----".to_vec(),
    )]);
    assert_eq!(
        parse(Format::ProtonPass, &pgp, &options())
            .unwrap_err()
            .code(),
        "import_proton_encrypted"
    );
}

// --- Detection and limits ---

#[test]
fn every_fixture_is_detected() {
    let cases: [(&str, Vec<u8>, Format); 7] = [
        ("lastpass.csv", fixture("lastpass.csv"), Format::LastPass),
        ("nordpass.csv", fixture("nordpass.csv"), Format::NordPass),
        ("passwords.csv", fixture("chrome.csv"), Format::Csv),
        (
            "credentials.csv",
            fixture("dashlane/credentials.csv"),
            Format::Dashlane,
        ),
        ("export.json", fixture("bitwarden.json"), Format::Bitwarden),
        ("keeper.json", fixture("keeper.json"), Format::Keeper),
        ("dashlane.json", fixture("dashlane.json"), Format::Dashlane),
    ];
    for (name, bytes, format) in cases {
        assert_eq!(
            detect(name, &bytes),
            Some(Detected {
                format,
                needs_password: false
            }),
            "{name}"
        );
    }
    assert_eq!(detect("x.json", br#"{"unknown": 1}"#), None);
    assert_eq!(
        detect("x.pwmx", b"PWMX\x01rest"),
        Some(Detected {
            format: Format::Pwm,
            needs_password: true
        })
    );
}

#[test]
fn hostile_files_stop_at_the_limits() {
    let huge = vec![b'a'; MAX_FILE_BYTES + 1];
    assert!(matches!(
        parse(Format::Csv, &huge, &options()),
        Err(ImportError::TooLarge)
    ));
    assert_eq!(detect("x.csv", &huge), None);

    let mut many = String::from("name,password\n");
    for i in 0..50_002 {
        many.push_str(&format!("n{i},p\n"));
    }
    assert!(matches!(
        parse(Format::Csv, many.as_bytes(), &options()),
        Err(ImportError::TooLarge)
    ));

    let deep = format!("{{\"items\": {}{}}}", "[".repeat(200), "]".repeat(200));
    assert!(matches!(
        parse(Format::Bitwarden, deep.as_bytes(), &options()),
        Err(ImportError::Unreadable)
    ));

    for format in Format::ALL {
        for bytes in [
            &b""[..],
            b"\x00\x01\x02",
            b"PK\x03\x04junk",
            b"{",
            b"a,b\n\"unclosed",
        ] {
            // Never a panic; an error or an empty import.
            let _ = parse(format, bytes, &options());
        }
    }
}

#[test]
fn errors_never_quote_the_file() {
    let secret = "hunter2-synthetic";
    let file =
        format!("{{\"encrypted\": false, \"items\": [{{\"type\": 1, \"login\": \"{secret}\"");
    let err = parse(Format::Bitwarden, file.as_bytes(), &options()).unwrap_err();
    assert!(!format!("{err} {err:?}").contains(secret));
}
