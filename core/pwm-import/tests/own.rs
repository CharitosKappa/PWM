//! Our own exports into any account (D51 §4, the D24 deferral): `.pwmx`,
//! JSON and CSV come back with new ids, folder references follow, and the
//! content is unchanged.

use pwm_crypto::{Id, KdfParams, Secret, create_account};
use pwm_export::{ExportItem, ExportVault, authorize_export, export_csv, export_json, export_pwmx};
use pwm_import::{Detected, Format, ImportError, Options, detect, parse};
use pwm_vault::{Folder, Item, ItemId, ItemKind, Login};

const ACCOUNT: Id = [0xA1; 16];
const MASTER: &str = "correct horse battery staple";
const EXPORT_PASSWORD: &str = "orbit-velvet-candle-harbor-ninth";
const NOW: i64 = 1_767_225_600_000;

fn grant() -> pwm_export::ExportGrant {
    let a = create_account(MASTER, &ACCOUNT, KdfParams::MOBILE_FLOOR).unwrap();
    authorize_export(
        MASTER,
        &a.secret_key,
        &ACCOUNT,
        &a.salt,
        &a.params,
        &a.sealed_keyset,
        NOW,
    )
    .unwrap()
}

/// A folder and a login inside it.
fn vault() -> Vec<ExportVault> {
    let folder_id = ItemId::from_bytes([1; 16]);
    let folder = Item::new(ItemKind::Folder(Folder::default()), "Work", NOW);
    let mut login = Login::default();
    login.username = "ada".into();
    login.password = "hunter2-synthetic".into();
    let mut item = Item::new(ItemKind::Login(login), "Bank", NOW);
    item.folder_id = Some(folder_id);
    item.notes = "Synthetic note".into();
    vec![ExportVault {
        id: [9; 16],
        items: vec![
            ExportItem {
                id: folder_id,
                item: folder,
            },
            ExportItem {
                id: ItemId::from_bytes([2; 16]),
                item,
            },
        ],
    }]
}

fn check(parsed: &pwm_import::Parsed) {
    assert_eq!(parsed.items.len(), 2);
    let (folder_id, folder) = &parsed.items[0];
    let (login_id, login) = &parsed.items[1];
    assert!(matches!(folder.kind, ItemKind::Folder(_)), "folders first");
    assert_ne!(*folder_id, ItemId::from_bytes([1; 16]), "new ids");
    assert_ne!(*login_id, ItemId::from_bytes([2; 16]));
    assert_eq!(login.folder_id, Some(*folder_id), "the reference follows");
    assert_eq!(login.title, "Bank");
    assert_eq!(login.notes, "Synthetic note");
    let ItemKind::Login(l) = &login.kind else {
        panic!("not a login")
    };
    assert_eq!(l.password, "hunter2-synthetic");
    assert_eq!((parsed.report.logins, parsed.report.folders), (1, 1));
}

#[test]
fn json_and_csv_exports_come_back_with_new_ids() {
    let json = export_json(grant(), NOW, &vault()).unwrap();
    assert_eq!(
        detect("export.json", json.expose_secret()).map(|d| d.format),
        Some(Format::Pwm)
    );
    let parsed = parse(
        Format::Pwm,
        json.expose_secret(),
        &Options {
            now_ms: NOW,
            ..Options::default()
        },
    )
    .unwrap();
    check(&parsed);
    // Unchanged apart from the ids.
    assert_eq!(parsed.items[1].1.created_at, NOW);

    let csv = export_csv(grant(), NOW, &vault()).unwrap();
    assert_eq!(
        detect("export.csv", csv.expose_secret()).map(|d| d.format),
        Some(Format::Pwm)
    );
    check(
        &parse(
            Format::Pwm,
            csv.expose_secret(),
            &Options {
                now_ms: NOW,
                ..Options::default()
            },
        )
        .unwrap(),
    );
}

#[test]
fn a_pwmx_needs_its_password() {
    let file = export_pwmx(grant(), NOW, &vault(), EXPORT_PASSWORD).unwrap();
    assert_eq!(
        detect("vault.pwmx", &file),
        Some(Detected {
            format: Format::Pwm,
            needs_password: true
        })
    );
    let with = |p: &str| Options {
        password: Some(Secret::new(p.to_owned())),
        now_ms: NOW,
        ..Options::default()
    };
    check(&parse(Format::Pwm, &file, &with(EXPORT_PASSWORD)).unwrap());
    assert!(matches!(
        parse(Format::Pwm, &file, &Options::default()),
        Err(ImportError::PasswordNeeded)
    ));
    assert!(matches!(
        parse(Format::Pwm, &file, &with("not the export password")),
        Err(ImportError::WrongPassword)
    ));
}
