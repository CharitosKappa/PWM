//! Passkeys through Android's Credential Manager (D61): an app gets the
//! site's passkeys only when the site's Digital Asset Links file lists it,
//! its new passkey links the login to it, and a privileged browser's hash is
//! signed as given.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_client::{
    AndroidPasskeyCaller, Client, ClientError, MemoryStore, PasskeyCreateCheck, Store, Tokens,
    passkey_asset_links_url,
};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, VaultRecord};
use pwm_vault::ItemId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";
const PACKAGE: &str = "com.example.app";
const CERT: [u8; 32] = [0xAB; 32];

fn unlocked() -> (Client<MemoryStore>, String) {
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
    let secret_key = new.secret_key.to_text().expose_secret().clone();
    let mut client = Client::new(store);
    client
        .unlock(
            PASSWORD,
            &secret_key,
            Tokens::new("pwma_x".to_owned(), "pwmr_x".to_owned()),
        )
        .unwrap();
    (client, ItemId::from_bytes(vault_id).to_string())
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(text).unwrap()
}

fn create(user: &str, exclude: &[String]) -> String {
    json!({
        "challenge": b64(b"challenge-create"),
        "rp": {"id": "example.com", "name": "Example"},
        "user": {"id": b64(user.as_bytes()), "name": user, "displayName": user},
        "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
        "excludeCredentials": exclude.iter().map(|id| json!({"type": "public-key", "id": id})).collect::<Vec<_>>(),
    })
    .to_string()
}

fn get() -> String {
    json!({"challenge": b64(b"challenge-get"), "rpId": "example.com"}).to_string()
}

fn fingerprint(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// The site's file, listing `package` signed with `cert`.
fn listing(package: &str, cert: &[u8]) -> String {
    json!([{
        "relation": ["delegate_permission/common.get_login_creds"],
        "target": {"namespace": "android_app", "package_name": package,
                   "sha256_cert_fingerprints": [fingerprint(cert)]},
    }])
    .to_string()
}

fn app(asset_links: Vec<String>) -> AndroidPasskeyCaller {
    AndroidPasskeyCaller {
        package: PACKAGE.to_owned(),
        cert_sha256: CERT.to_vec(),
        origin: None,
        client_data_hash: None,
        asset_links,
    }
}

fn browser(origin: &str, hash: Option<Vec<u8>>) -> AndroidPasskeyCaller {
    AndroidPasskeyCaller {
        package: "com.android.chrome".to_owned(),
        cert_sha256: vec![0xF0; 32],
        origin: Some(origin.to_owned()),
        client_data_hash: hash,
        asset_links: Vec::new(),
    }
}

fn items(client: &Client<MemoryStore>, vault: &str) -> Vec<Value> {
    client
        .items(vault)
        .unwrap()
        .iter()
        .map(|j| serde_json::from_str(j.expose_secret()).unwrap())
        .collect()
}

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> &'static str {
    result.unwrap_err().code()
}

fn link() -> String {
    format!("android-app://{PACKAGE}?sha256={}", hex::encode(CERT))
}

#[test]
fn the_file_to_fetch_is_the_rp_ids() {
    assert_eq!(
        passkey_asset_links_url(&create("ada", &[]), true).unwrap(),
        "https://example.com/.well-known/assetlinks.json"
    );
    assert_eq!(
        passkey_asset_links_url(&get(), false).unwrap(),
        "https://example.com/.well-known/assetlinks.json"
    );
    // An app must name its RP id.
    let unnamed = json!({"challenge": "AAAA"}).to_string();
    assert_eq!(
        code(passkey_asset_links_url(&unnamed, false)),
        "invalid_passkey_request"
    );
}

#[test]
fn an_app_the_site_does_not_list_gets_nothing() {
    let (mut client, vault) = unlocked();
    for files in [
        vec![],
        vec![listing("com.example.other", &CERT)],
        vec![listing(PACKAGE, &[0xCD; 32])],
        vec!["not json".to_owned()],
    ] {
        let caller = app(files);
        assert_eq!(
            code(client.passkey_create_check_android(&caller, &create("ada", &[]))),
            "passkey_app_not_linked"
        );
        assert_eq!(
            code(client.passkey_create_android(&vault, &caller, &create("ada", &[]), T0)),
            "passkey_app_not_linked"
        );
        assert_eq!(
            code(client.passkey_candidates_android(&caller, &get())),
            "passkey_app_not_linked"
        );
    }
    assert!(items(&client, &vault).is_empty());
}

#[test]
fn a_listed_app_makes_a_passkey_that_links_the_login_to_it() {
    let (mut client, vault) = unlocked();
    let caller = app(vec![listing(PACKAGE, &CERT)]);
    assert_eq!(
        client
            .passkey_create_check_android(&caller, &create("ada", &[]))
            .unwrap(),
        PasskeyCreateCheck::Ready {
            rp_id: "example.com".into(),
            user_name: "ada".into(),
        }
    );
    let response: Value = serde_json::from_str(
        &client
            .passkey_create_android(&vault, &caller, &create("ada", &[]), T0)
            .unwrap(),
    )
    .unwrap();
    let client_data: Value = serde_json::from_slice(&unb64(
        response["response"]["clientDataJSON"].as_str().unwrap(),
    ))
    .unwrap();
    assert_eq!(
        client_data["origin"],
        format!("android:apk-key-hash:{}", b64(&CERT))
    );
    assert_eq!(client_data["androidPackageName"], PACKAGE);

    let all = items(&client, &vault);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["title"], "example.com");
    let urls: Vec<(String, String)> = all[0]["login"]["urls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u["url"].as_str().unwrap().to_owned(),
                u["match"].as_str().unwrap_or("domain").to_owned(),
            )
        })
        .collect();
    assert_eq!(
        urls,
        vec![
            ("https://example.com".to_owned(), "domain".to_owned()),
            (link(), "exact".to_owned()),
        ]
    );

    let candidates = client.passkey_candidates_android(&caller, &get()).unwrap();
    assert_eq!(candidates.len(), 1);
    let c = &candidates[0];
    let assertion: Value = serde_json::from_str(
        &client
            .passkey_get_android(&caller, &get(), &c.vault_id, &c.item_id, &c.credential_id)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(assertion["id"], response["id"]);
    let client_data: Value = serde_json::from_slice(&unb64(
        assertion["response"]["clientDataJSON"].as_str().unwrap(),
    ))
    .unwrap();
    assert_eq!(client_data["type"], "webauthn.get");

    // The passkey is the site's: the browser finds it too.
    assert_eq!(
        client
            .passkey_candidates_android(&browser("https://example.com", Some(vec![1; 32])), &get())
            .unwrap()
            .len(),
        1
    );
    // Excluded once it exists.
    assert_eq!(
        client
            .passkey_create_check_android(
                &caller,
                &create("ada", &[response["id"].as_str().unwrap().to_owned()])
            )
            .unwrap(),
        PasskeyCreateCheck::Excluded
    );
}

#[test]
fn a_saved_login_gets_the_passkey_and_the_link_once() {
    let (mut client, vault) = unlocked();
    client
        .save_login(&vault, "https://example.com/login", "", "ada", "pw", T0)
        .unwrap();
    let caller = app(vec![listing(PACKAGE, &CERT)]);
    client
        .passkey_create_android(&vault, &caller, &create("ada", &[]), T0 + 1)
        .unwrap();
    client
        .passkey_create_android(&vault, &caller, &create("ada", &[]), T0 + 2)
        .unwrap();
    let all = items(&client, &vault);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["login"]["password"], "pw");
    assert_eq!(all[0]["login"]["passkeys"].as_array().unwrap().len(), 2);
    let links = all[0]["login"]["urls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|u| u["url"] == link())
        .count();
    assert_eq!(links, 1);
}

#[test]
fn an_included_file_can_list_the_app() {
    let (client, _) = unlocked();
    let site =
        json!([{"include": "https://other.example/.well-known/assetlinks.json"}]).to_string();
    let caller = app(vec![site, listing(PACKAGE, &CERT)]);
    assert!(matches!(
        client
            .passkey_create_check_android(&caller, &create("ada", &[]))
            .unwrap(),
        PasskeyCreateCheck::Ready { .. }
    ));
}

#[test]
fn a_browser_hash_is_signed_and_its_origin_checked() {
    let (mut client, vault) = unlocked();
    let hash: Vec<u8> = Sha256::digest(b"chrome's client data").to_vec();
    let chrome = browser("https://login.example.com", Some(hash.clone()));
    let response: Value = serde_json::from_str(
        &client
            .passkey_create_android(&vault, &chrome, &create("ada", &[]), T0)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(response["response"]["clientDataJSON"], "e30");
    let all = items(&client, &vault);
    assert_eq!(
        all[0]["login"]["urls"][0]["url"],
        "https://login.example.com"
    );
    assert_eq!(all[0]["login"]["urls"].as_array().unwrap().len(), 1);

    let c = &client.passkey_candidates_android(&chrome, &get()).unwrap()[0];
    let assertion: Value = serde_json::from_str(
        &client
            .passkey_get_android(&chrome, &get(), &c.vault_id, &c.item_id, &c.credential_id)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(assertion["response"]["clientDataJSON"], "e30");

    // Without a hash, the client data is built for the browser's origin.
    let plain = browser("https://login.example.com", None);
    let built: Value = serde_json::from_str(
        &client
            .passkey_get_android(&plain, &get(), &c.vault_id, &c.item_id, &c.credential_id)
            .unwrap(),
    )
    .unwrap();
    let data: Value = serde_json::from_slice(&unb64(
        built["response"]["clientDataJSON"].as_str().unwrap(),
    ))
    .unwrap();
    assert_eq!(data["origin"], "https://login.example.com");

    // Another site's origin, and a plain-HTTP one.
    assert_eq!(
        code(client.passkey_candidates_android(
            &browser("https://other.example", Some(hash.clone())),
            &get()
        )),
        "invalid_rp_id"
    );
    assert_eq!(
        code(client.passkey_create_android(
            &vault,
            &browser("http://example.com", Some(hash)),
            &create("bob", &[]),
            T0
        )),
        "insecure_origin"
    );
}
