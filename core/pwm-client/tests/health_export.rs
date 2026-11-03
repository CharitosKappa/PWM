//! The health report (D22) and export (D24) through the client, as the apps
//! call them (D43 §7–8): across every vault, without a server.

use pwm_client::{Client, ClientError, ExportFormat, MemoryStore, Tokens};
use pwm_crypto::{KdfParams, SymmetricKey, create_account, hibp_sha1};
use pwm_store::{AccountRecord, Store, VaultRecord};
use pwm_vault::{Item, ItemId, ItemKind, Login};
use serde_json::Value;

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";

/// An unlocked client with two vaults; returns it with its Secret Key and
/// the vault ids.
fn unlocked() -> (Client<MemoryStore>, String, [String; 2]) {
    let account_id = *ItemId::generate().unwrap().as_bytes();
    let new = create_account(PASSWORD, &account_id, KdfParams::MOBILE_FLOOR).unwrap();
    let mut store = MemoryStore::new();
    store
        .put_account(&AccountRecord {
            account_id,
            salt: new.salt,
            kdf_params: new.params,
            sealed_keyset: new.sealed_keyset,
        })
        .unwrap();
    let vaults = [0, 1].map(|_| {
        let vault_id = *ItemId::generate().unwrap().as_bytes();
        let wrapped = new
            .keyset
            .wrap_vault_key(&SymmetricKey::generate().unwrap(), &vault_id)
            .unwrap();
        store
            .put_vault(&VaultRecord {
                vault_id,
                wrapped_key: wrapped,
                sync_cursor: 0,
            })
            .unwrap();
        ItemId::from_bytes(vault_id).to_string()
    });
    let secret_key = new.secret_key.to_text().expose_secret().clone();
    let mut client = Client::new(store);
    client
        .unlock(
            PASSWORD,
            &secret_key,
            Tokens::new("pwma_x".into(), "pwmr_x".into()),
        )
        .unwrap();
    (client, secret_key, vaults)
}

/// Saves a login with this password; returns its id.
fn login(client: &mut Client<MemoryStore>, vault: &str, title: &str, password: &str) -> String {
    let id = ItemId::generate().unwrap();
    let mut login = Login::default();
    login.username = "alice".into();
    login.password = password.into();
    let json = Item::new(ItemKind::Login(login), title, T0)
        .to_json_with_id(&id)
        .unwrap();
    client.save_item(vault, json.expose_secret(), T0).unwrap()
}

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> &'static str {
    result.unwrap_err().code()
}

#[test]
fn weak_and_reused_passwords_across_vaults() {
    let (mut client, _, [first, second]) = unlocked();
    let strong = "vivid-orbit-tundra-marble-42";
    let a = login(&mut client, &first, "Bank", strong);
    let b = login(&mut client, &second, "Shop", strong);
    let weak = login(&mut client, &first, "Forum", "alice2024");
    login(
        &mut client,
        &second,
        "Mail",
        "a different strong passphrase 77",
    );

    let report = client.health_report().unwrap();
    assert_eq!(report.weak.len(), 1);
    assert_eq!(report.weak[0].item_id, weak);
    assert!(report.weak[0].score < 3);
    let mut pair = vec![a, b];
    pair.sort();
    assert_eq!(report.reused, vec![pair], "found across vaults");

    client.lock();
    assert_eq!(code(client.health_report()), "locked");
}

#[test]
fn the_breach_check_sends_only_prefixes() {
    let (mut client, _, [first, second]) = unlocked();
    let leaked = login(&mut client, &first, "Old", "password123456");
    login(&mut client, &second, "New", "vivid-orbit-tundra-marble-42");
    let hash = hibp_sha1(b"password123456");
    let hex: String = hash
        .expose_secret()
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();

    let mut check = client.breach_check().unwrap();
    let prefixes = check.prefixes();
    assert_eq!(prefixes.len(), 2);
    assert!(prefixes.contains(&hex[..5].to_owned()));
    assert!(prefixes.iter().all(|p| p.len() == 5));
    for prefix in &prefixes {
        let body = if *prefix == hex[..5] {
            format!(
                "{}:3730471\r\n0000000000000000000000000000000000A:0\r\n",
                &hex[5..]
            )
        } else {
            "0000000000000000000000000000000000A:0\r\n".to_owned()
        };
        check.add_response(prefix, &body).unwrap();
    }
    assert_eq!(
        code(check.add_response(&prefixes[0], "not a range")),
        "protocol"
    );
    let breached = check.finish();
    assert_eq!(breached.len(), 1);
    assert_eq!(breached[0].item_id, leaked);
    assert_eq!(breached[0].count, 3_730_471);
    assert!(!format!("{:?}", client.breach_check().unwrap()).contains(&hex[..5]));
}

fn exported(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn every_export_asks_for_the_master_password() {
    let (mut client, secret_key, [first, _]) = unlocked();
    login(&mut client, &first, "Bank", "hunter2 hunter2");

    // No grant, or a wrong password: nothing.
    assert_eq!(
        code(client.export(ExportFormat::Json, T0)),
        "export_not_authorized"
    );
    assert_eq!(
        code(client.authorize_export("not the password at all", &secret_key, T0)),
        "wrong_password"
    );
    assert_eq!(
        code(client.export(ExportFormat::Json, T0)),
        "export_not_authorized"
    );

    // A grant works once.
    client.authorize_export(PASSWORD, &secret_key, T0).unwrap();
    let json = client.export(ExportFormat::Json, T0 + 1).unwrap();
    let parsed = exported(json.expose_secret());
    assert_eq!(parsed["vaults"].as_array().unwrap().len(), 2);
    assert!(String::from_utf8_lossy(json.expose_secret()).contains("hunter2 hunter2"));
    assert_eq!(
        code(client.export(ExportFormat::Csv, T0 + 2)),
        "export_not_authorized"
    );

    // CSV, then a grant that is too old.
    client.authorize_export(PASSWORD, &secret_key, T0).unwrap();
    let csv = client.export(ExportFormat::Csv, T0 + 1).unwrap();
    assert!(String::from_utf8_lossy(csv.expose_secret()).contains("hunter2 hunter2"));
    client.authorize_export(PASSWORD, &secret_key, T0).unwrap();
    assert_eq!(
        code(client.export(ExportFormat::Json, T0 + 5 * 60 * 1000 + 1)),
        "export_expired"
    );

    // Locking drops a grant.
    client.authorize_export(PASSWORD, &secret_key, T0).unwrap();
    client.lock();
    client
        .unlock(PASSWORD, &secret_key, Tokens::new("a".into(), "b".into()))
        .unwrap();
    assert_eq!(
        code(client.export(ExportFormat::Json, T0)),
        "export_not_authorized"
    );
}

#[test]
fn an_encrypted_export_needs_a_strong_export_password() {
    let (mut client, secret_key, [first, _]) = unlocked();
    login(&mut client, &first, "Bank", "hunter2 hunter2");
    client.authorize_export(PASSWORD, &secret_key, T0).unwrap();

    // A weak export password does not use up the grant.
    assert_eq!(
        code(client.export(ExportFormat::Pwmx("password"), T0)),
        "weak_export_password"
    );
    let file = client
        .export(ExportFormat::Pwmx("granite-sparrow-lattice-91"), T0)
        .unwrap();
    assert!(!String::from_utf8_lossy(file.expose_secret()).contains("hunter2"));
    let opened = pwm_export::read_pwmx(file.expose_secret(), "granite-sparrow-lattice-91").unwrap();
    assert_eq!(opened.vaults.len(), 2);
    assert_eq!(
        opened.vaults.iter().map(|v| v.items.len()).sum::<usize>(),
        1
    );
}
