//! Item model v1 (decision D18): types, forward compatibility, padding,
//! encryption binding, validation and redaction.

use pwm_crypto::{Envelope, Id, Secret, SymmetricKey};
use pwm_vault::{
    Card, CustomField, FieldKind, Folder, Identity, Item, ItemId, ItemKind, Login, LoginUrl,
    Passkey, SCHEMA_VERSION, UrlMatch, VaultError, collect_tags, decrypt_item, encrypt_item,
    remove_folder_refs,
};
use serde_json::Value;

const VAULT: Id = [0xB1; 16];
const NOW: i64 = 1_767_225_600_000;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/items_v1.json")).unwrap()
}

fn vault_key() -> SymmetricKey {
    SymmetricKey::from_secret(Secret::new([0x34; 32]))
}

fn item_id(n: u8) -> ItemId {
    ItemId::from_bytes([n; 16])
}

/// Every key/value in `original` must appear unchanged in `output`.
fn assert_subset(original: &Value, output: &Value, path: &str) {
    match (original, output) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                let got = b.get(k).unwrap_or_else(|| panic!("{path}.{k} dropped"));
                assert_subset(v, got, &format!("{path}.{k}"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path} length");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                assert_subset(x, y, &format!("{path}[{i}]"));
            }
        }
        _ => assert_eq!(original, output, "{path}"),
    }
}

fn sample_items() -> Vec<Item> {
    // Types keep unknown fields privately, so build them from defaults.
    let mut login = Login::default();
    login.username = "alice@example.com".into();
    login.password = "hunter2".into();
    login.totp = Some("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP".into());
    let mut url = LoginUrl::default();
    url.url = "https://example.com".into();
    url.r#match = UrlMatch::Host;
    login.urls.push(url);
    let mut passkey = Passkey::default();
    passkey.credential_id = vec![1, 2, 3];
    passkey.rp_id = "example.com".into();
    passkey.user_handle = vec![4, 5];
    passkey.user_name = "alice".into();
    passkey.private_key = vec![9; 48];
    passkey.sign_count = 7;
    passkey.created_at = NOW;
    login.passkeys.push(passkey);
    let mut with_fields = Item::new(ItemKind::Login(login), "Example", NOW);
    with_fields.tags = vec!["work".into()];
    with_fields.favorite = true;
    with_fields.folder_id = Some(item_id(9));
    let mut field = CustomField::default();
    field.name = "PIN".into();
    field.value = "1234".into();
    field.kind = FieldKind::Hidden;
    with_fields.custom_fields.push(field);

    let mut card = Card::default();
    card.cardholder = "Alice".into();
    card.number = "4111111111111111".into();
    card.expiry_month = Some(12);
    card.expiry_year = Some(2030);
    card.security_code = "123".into();
    card.brand = "visa".into();
    let mut identity = Identity::default();
    identity.first_name = "Alice".into();
    identity.last_name = "Example".into();
    identity.address_lines = vec!["1 Main St".into()];
    identity.country = "GR".into();
    let mut folder = Folder::default();
    folder.parent_id = Some(item_id(8));
    vec![
        with_fields,
        Item::new(ItemKind::Note, "Note", NOW),
        Item::new(ItemKind::Card(card), "Card", NOW),
        Item::new(ItemKind::Identity(identity), "Identity", NOW),
        Item::new(ItemKind::Folder(folder), "Folder", NOW),
    ]
}

// --- Schema and forward compatibility ---

#[test]
fn v1_fixtures_parse_and_keep_every_field() {
    for (i, fixture) in fixtures().iter().enumerate() {
        let json = fixture.to_string();
        let item = Item::from_json(&json).unwrap_or_else(|e| panic!("fixture {i}: {e}"));
        let out: Value = serde_json::from_str(item.to_json().unwrap().expose_secret()).unwrap();
        assert_subset(fixture, &out, &format!("fixture[{i}]"));
        let again = Item::from_json(&out.to_string()).unwrap();
        assert_eq!(again, item, "fixture {i} round trip");
    }
}

#[test]
fn fixture_login_fields() {
    let item = Item::from_json(&fixtures()[0].to_string()).unwrap();
    assert_eq!(item.schema_version(), 1);
    assert!(item.is_editable());
    assert_eq!(item.title, "Example");
    assert_eq!(item.tags, ["work", "email"]);
    assert_eq!(item.custom_fields[0].kind, FieldKind::Hidden);
    assert_eq!(
        item.folder_id.unwrap().to_string(),
        "0f0e0d0c-0b0a-4908-8706-050403020100"
    );
    let ItemKind::Login(login) = &item.kind else {
        panic!("not a login")
    };
    assert_eq!(login.username, "alice@example.com");
    assert_eq!(login.urls[0].r#match, UrlMatch::Domain);
    assert_eq!(login.urls[1].r#match, UrlMatch::StartsWith);
    assert_eq!(login.passkeys[0].credential_id, [1, 2, 3, 4]);
    assert_eq!(login.passkeys[0].sign_count, 3);
}

#[test]
fn unknown_enum_values_are_kept() {
    let item = Item::from_json(&fixtures()[5].to_string()).unwrap();
    assert!(item.is_editable());
    assert_eq!(item.custom_fields[0].kind, FieldKind::Other("otp".into()));
    let ItemKind::Login(login) = &item.kind else {
        panic!("not a login")
    };
    assert_eq!(login.urls[0].r#match, UrlMatch::Other("fuzzy".into()));
}

#[test]
fn unknown_item_type_is_read_only() {
    let item = Item::from_json(&fixtures()[6].to_string()).unwrap();
    assert_eq!(item.kind, ItemKind::Unknown("wifi".into()));
    assert!(!item.is_editable());
    assert_eq!(item.title, "Home network (type from a newer app version)");
}

#[test]
fn newer_schema_is_read_only_and_cannot_be_encrypted() {
    let item = Item::from_json(&fixtures()[7].to_string()).unwrap();
    assert_eq!(item.schema_version(), 2);
    assert!(!item.is_editable());
    assert!(matches!(
        encrypt_item(&vault_key(), &VAULT, &item_id(1), 1, &item),
        Err(VaultError::ReadOnly)
    ));
    let unknown = Item::from_json(&fixtures()[6].to_string()).unwrap();
    assert!(matches!(
        encrypt_item(&vault_key(), &VAULT, &item_id(1), 1, &unknown),
        Err(VaultError::ReadOnly)
    ));
}

#[test]
fn new_items_use_the_current_schema() {
    assert_eq!(SCHEMA_VERSION, 1);
    let item = Item::new(ItemKind::Note, "n", NOW);
    assert_eq!(item.schema_version(), SCHEMA_VERSION);
    assert_eq!((item.created_at, item.updated_at), (NOW, NOW));
    let json: Value = serde_json::from_str(item.to_json().unwrap().expose_secret()).unwrap();
    assert_eq!(json["v"], 1);
    assert_eq!(json["type"], "note");
}

#[test]
fn malformed_json_is_rejected_without_leaking_input() {
    let cases = [
        r#"{"v":1,"type":"login","title":["SUPERSECRET"],"created_at":0,"updated_at":0,"login":{}}"#,
        r#"{"v":1,"type":"login","title":"t","created_at":0,"updated_at":0,"login":{"password":["SUPERSECRET"]}}"#,
        r#"{"v":1,"type":"login","title":"t","created_at":0,"updated_at":0}"#, // payload missing
        r#"{"v":0,"type":"note","title":"SUPERSECRET","created_at":0,"updated_at":0}"#,
        r#"{"type":"note","title":"SUPERSECRET","created_at":0,"updated_at":0}"#,
        r#"{"v":1,"type":"note","title":"t","folder_id":"SUPERSECRET","created_at":0,"updated_at":0}"#,
        r#"{"v":1,"type":"login","title":"t","created_at":0,"updated_at":0,"login":{"passkeys":[{"private_key":"SUPERSECRET!!"}]}}"#,
        "SUPERSECRET",
    ];
    for json in cases {
        let err = Item::from_json(json).unwrap_err();
        assert!(matches!(err, VaultError::InvalidItem), "{json}");
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains("SUPERSECRET"), "{shown}");
    }
}

// --- Encryption, padding, binding ---

#[test]
fn every_type_round_trips_through_encryption() {
    let key = vault_key();
    for (n, item) in sample_items().into_iter().enumerate() {
        let id = item_id(n as u8);
        let env = encrypt_item(&key, &VAULT, &id, 5, &item).unwrap();
        let back = decrypt_item(&key, &VAULT, &id, 5, &env).unwrap();
        assert_eq!(back, item);
    }
}

#[test]
fn plaintext_is_padded_to_256_byte_buckets() {
    let key = vault_key();
    let sizes: Vec<usize> = sample_items()
        .iter()
        .map(|item| {
            let env = encrypt_item(&key, &VAULT, &item_id(1), 1, item).unwrap();
            env.to_bytes().len() - Envelope::MIN_LEN
        })
        .collect();
    for size in &sizes {
        assert_eq!(size % 256, 0, "{sizes:?}");
    }
    // A short note and a folder are indistinguishable by size.
    assert_eq!(sizes[1], 256);
    assert_eq!(sizes[4], 256);

    let mut long = Item::new(ItemKind::Note, "long", NOW);
    long.notes = "x".repeat(1000);
    let env = encrypt_item(&key, &VAULT, &item_id(1), 1, &long).unwrap();
    assert_eq!(env.to_bytes().len() - Envelope::MIN_LEN, 1280);
}

#[test]
fn ciphertext_is_bound_to_key_vault_item_and_revision() {
    let key = vault_key();
    let item = Item::new(ItemKind::Note, "bound", NOW);
    let env = encrypt_item(&key, &VAULT, &item_id(1), 5, &item).unwrap();
    let other_key = SymmetricKey::from_secret(Secret::new([0x35; 32]));
    let attempts = [
        decrypt_item(&other_key, &VAULT, &item_id(1), 5, &env),
        decrypt_item(&key, &[0xB2; 16], &item_id(1), 5, &env),
        decrypt_item(&key, &VAULT, &item_id(2), 5, &env),
        decrypt_item(&key, &VAULT, &item_id(1), 6, &env),
    ];
    for result in attempts {
        assert!(matches!(result, Err(VaultError::Crypto(_))));
    }
}

#[test]
fn validation_rejects_bad_values() {
    let key = vault_key();
    let mut card = Card::default();
    card.expiry_month = Some(13);
    let bad_card = Item::new(ItemKind::Card(card), "c", NOW);
    let mut empty_url = Login::default();
    empty_url.urls.push(LoginUrl::default());
    let bad_login = Item::new(ItemKind::Login(empty_url), "l", NOW);
    for item in [bad_card, bad_login] {
        assert!(matches!(
            encrypt_item(&key, &VAULT, &item_id(1), 1, &item),
            Err(VaultError::Invalid(_))
        ));
    }
}

// --- Helpers and hygiene ---

#[test]
fn debug_output_is_redacted() {
    for item in sample_items() {
        let out = format!("{item:?}");
        for secret in [
            "Example", "hunter2", "alice", "4111", "PIN", "work", "Alice",
        ] {
            assert!(!out.contains(secret), "{out}");
        }
    }
}

#[test]
fn ids_are_uuid_v4_strings() {
    let id = ItemId::generate().unwrap();
    let text = id.to_string();
    assert_eq!(text.len(), 36);
    assert_eq!(&text[14..15], "4");
    assert_eq!(text.parse::<ItemId>().unwrap(), id);
    assert_ne!(ItemId::generate().unwrap(), id);
    for bad in [
        "",
        "not-a-uuid",
        "0f0e0d0c0b0a49088706050403020100",
        "0f0e0d0c-0b0a-4908-8706-05040302010g",
    ] {
        assert!(bad.parse::<ItemId>().is_err(), "{bad}");
    }
    assert_eq!(item_id(0xAB).as_bytes(), &[0xAB; 16]);
}

#[test]
fn tags_are_collected_from_items() {
    let mut a = Item::new(ItemKind::Note, "a", NOW);
    a.tags = vec!["work".into(), "email".into()];
    let mut b = Item::new(ItemKind::Note, "b", NOW);
    b.tags = vec!["work".into(), "bank".into()];
    assert_eq!(collect_tags(&[a, b]), ["bank", "email", "work"]);
}

#[test]
fn deleting_a_folder_moves_items_to_no_folder() {
    let folder = item_id(7);
    let mut items = sample_items();
    items[1].folder_id = Some(folder);
    items[2].folder_id = Some(folder);
    items[3].folder_id = Some(item_id(6));
    assert_eq!(remove_folder_refs(&mut items, &folder), 2);
    assert_eq!(items[1].folder_id, None);
    assert_eq!(items[2].folder_id, None);
    assert_eq!(items[3].folder_id, Some(item_id(6)));
}

#[test]
fn json_with_id_round_trips_every_fixture() {
    for (n, raw) in fixtures().iter().enumerate() {
        let item = Item::from_json(&raw.to_string()).unwrap();
        let id = item_id(n as u8 + 1);
        let json = item.to_json_with_id(&id).unwrap();
        let value: Value = serde_json::from_str(json.expose_secret()).unwrap();
        assert_eq!(value["id"], id.to_string());
        assert!(json.expose_secret().starts_with("{\"id\":"));
        let (parsed_id, parsed) = Item::from_json_with_id(json.expose_secret()).unwrap();
        assert_eq!(parsed_id, id);
        assert!(parsed == item, "fixture {n} changed");
        // The id is not kept as an unknown field.
        assert_eq!(
            parsed.to_json().unwrap().expose_secret(),
            item.to_json().unwrap().expose_secret()
        );
    }
}

#[test]
fn json_with_id_needs_a_valid_id() {
    let raw = fixtures()[1].to_string();
    assert!(matches!(
        Item::from_json_with_id(&raw),
        Err(VaultError::InvalidItem)
    ));
    let bad = raw.replacen('{', "{\"id\":\"not-an-id\",", 1);
    assert!(matches!(
        Item::from_json_with_id(&bad),
        Err(VaultError::InvalidItem)
    ));
    let number = raw.replacen('{', "{\"id\":7,", 1);
    assert!(matches!(
        Item::from_json_with_id(&number),
        Err(VaultError::InvalidItem)
    ));
}
