//! Passkeys through the client (D37 §5): where a new passkey is stored,
//! which ones a sign-in may use, and that the vault's passkey signs.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_client::{
    Client, ClientError, CredentialIdentity, MemoryStore, PasskeyCreateCheck, Store, Tokens, new_id,
};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, VaultRecord};
use pwm_vault::{Item, ItemId, ItemKind, Login, LoginUrl, UrlMatch};
use serde_json::{Value, json};

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";
const ORIGIN: &str = "https://www.example.com";

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

fn get(allow: &[String]) -> String {
    json!({
        "challenge": b64(b"challenge-get"),
        "rpId": "example.com",
        "allowCredentials": allow.iter().map(|id| json!({"type": "public-key", "id": id})).collect::<Vec<_>>(),
    })
    .to_string()
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

#[test]
fn a_new_passkey_gets_a_new_login_and_signs_in() {
    let (mut client, vault) = unlocked();
    assert_eq!(
        client
            .passkey_create_check(ORIGIN, None, &create("ada", &[]))
            .unwrap(),
        PasskeyCreateCheck::Ready {
            rp_id: "example.com".into(),
            user_name: "ada".into(),
        }
    );
    let response = client
        .passkey_create(&vault, ORIGIN, None, &create("ada", &[]), T0)
        .unwrap();
    let response: Value = serde_json::from_str(&response).unwrap();
    let id = response["id"].as_str().unwrap().to_owned();

    let all = items(&client, &vault);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["title"], "example.com");
    assert_eq!(all[0]["login"]["username"], "ada");
    assert_eq!(all[0]["login"]["urls"][0]["url"], ORIGIN);
    let passkey = &all[0]["login"]["passkeys"][0];
    assert_eq!(passkey["credential_id"], id);
    assert_eq!(passkey["rp_id"], "example.com");
    assert_eq!(passkey["user_name"], "ada");
    assert_eq!(passkey["created_at"], T0);
    assert_eq!(passkey["sign_count"], 0);

    let candidates = client.passkey_candidates(ORIGIN, None, &get(&[])).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].credential_id, id);
    assert_eq!(candidates[0].user_name, "ada");
    let c = &candidates[0];
    let assertion = client
        .passkey_get(
            ORIGIN,
            None,
            &get(&[]),
            &c.vault_id,
            &c.item_id,
            &c.credential_id,
        )
        .unwrap();
    let assertion: Value = serde_json::from_str(&assertion).unwrap();
    assert_eq!(assertion["id"], id);
    assert_eq!(assertion["response"]["userHandle"], b64(b"ada"));
}

#[test]
fn a_passkey_joins_the_saved_login_for_that_username() {
    let (mut client, vault) = unlocked();
    let login = client
        .save_login(&vault, "https://example.com/login", "", "Ada", "pw", T0)
        .unwrap();
    client
        .passkey_create(&vault, ORIGIN, None, &create("ada", &[]), T0 + 1)
        .unwrap();
    let all = items(&client, &vault);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["id"], login);
    assert_eq!(all[0]["login"]["password"], "pw");
    assert_eq!(all[0]["login"]["passkeys"].as_array().unwrap().len(), 1);
    // Another username on the same site is another login.
    client
        .passkey_create(&vault, ORIGIN, None, &create("bob", &[]), T0 + 2)
        .unwrap();
    assert_eq!(items(&client, &vault).len(), 2);
}

#[test]
fn an_excluded_credential_of_ours_hands_the_request_back() {
    let (mut client, vault) = unlocked();
    let response = client
        .passkey_create(&vault, ORIGIN, None, &create("ada", &[]), T0)
        .unwrap();
    let id = serde_json::from_str::<Value>(&response).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        client
            .passkey_create_check(ORIGIN, None, &create("ada", &[id]))
            .unwrap(),
        PasskeyCreateCheck::Excluded
    );
    // Someone else's excluded credential does not matter.
    assert!(matches!(
        client
            .passkey_create_check(ORIGIN, None, &create("ada", &[b64(&[5; 16])]))
            .unwrap(),
        PasskeyCreateCheck::Ready { .. }
    ));
}

#[test]
fn no_es256_hands_the_request_back() {
    let (client, _) = unlocked();
    let request = json!({
        "challenge": "AAAA", "rp": {"name": "x"}, "user": {"id": "AA", "name": "a"},
        "pubKeyCredParams": [{"type": "public-key", "alg": -257}],
    })
    .to_string();
    assert_eq!(
        client.passkey_create_check(ORIGIN, None, &request).unwrap(),
        PasskeyCreateCheck::Unsupported
    );
}

#[test]
fn candidates_follow_the_rp_and_the_allow_list() {
    let (mut client, vault) = unlocked();
    let first = client
        .passkey_create(&vault, ORIGIN, None, &create("ada", &[]), T0)
        .unwrap();
    client
        .passkey_create(&vault, ORIGIN, None, &create("bob", &[]), T0)
        .unwrap();
    let first_id = serde_json::from_str::<Value>(&first).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        client
            .passkey_candidates(ORIGIN, None, &get(&[]))
            .unwrap()
            .len(),
        2
    );
    let only = client
        .passkey_candidates(ORIGIN, None, &get(std::slice::from_ref(&first_id)))
        .unwrap();
    assert_eq!(only.len(), 1);
    assert_eq!(only[0].user_name, "ada");
    // Another site sees none of them; a sub-domain of the RP sees them all.
    assert!(
        client
            .passkey_candidates(
                "https://other.example",
                None,
                &json!({"challenge": "AA"}).to_string()
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        client
            .passkey_candidates("https://shop.example.com", None, &get(&[]))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn refusals_have_codes() {
    let (mut client, vault) = unlocked();
    assert_eq!(
        code(client.passkey_create_check("http://example.com", None, &create("a", &[]))),
        "insecure_origin"
    );
    assert_eq!(
        code(client.passkey_create_check("https://evil.example", None, &create("a", &[]))),
        "invalid_rp_id"
    );
    assert_eq!(
        code(client.passkey_create_check(ORIGIN, None, "{}")),
        "invalid_passkey_request"
    );
    assert_eq!(
        code(client.passkey_get(ORIGIN, None, &get(&[]), &vault, &new_id().unwrap(), "AA")),
        "unknown_item"
    );
    client.lock();
    assert_eq!(
        code(client.passkey_candidates(ORIGIN, None, &get(&[]))),
        "locked"
    );
    assert_eq!(
        code(client.passkey_create(&vault, ORIGIN, None, &create("a", &[]), T0)),
        "locked"
    );
}

// --- Hashed calls (D47 §6) and the identities iOS suggests (D47 §5) ---

const HASH: [u8; 32] = [0x5A; 32];

#[test]
fn a_hashed_passkey_is_stored_and_signs() {
    let (mut client, vault) = unlocked();
    let created = client
        .passkey_register_hashed(&vault, "example.com", "ada", b"handle-ada", T0)
        .unwrap();
    assert_eq!(created.credential_id.len(), 16);
    // The attestation object carries the RP id hash and the credential id.
    assert!(
        created
            .attestation_object
            .windows(16)
            .any(|w| w == created.credential_id.as_slice())
    );

    let all = items(&client, &vault);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["title"], "example.com");
    assert_eq!(all[0]["login"]["username"], "ada");
    assert_eq!(all[0]["login"]["urls"][0]["url"], "https://example.com");
    let passkey = &all[0]["login"]["passkeys"][0];
    assert_eq!(passkey["credential_id"], b64(&created.credential_id));
    assert_eq!(passkey["user_handle"], b64(b"handle-ada"));
    let item_id = all[0]["id"].as_str().unwrap();

    let signed = client
        .passkey_assert_hashed(
            &vault,
            item_id,
            &created.credential_id,
            "example.com",
            &HASH,
        )
        .unwrap();
    assert_eq!(signed.authenticator_data.len(), 37);
    assert!(!signed.signature.is_empty());
    assert_eq!(signed.user_handle, b"handle-ada");
    assert_eq!(signed.credential_id, created.credential_id);
    // The browser path sees the same passkey.
    assert_eq!(
        client
            .passkey_candidates("https://example.com", None, &get(&[]))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_hashed_passkey_joins_the_saved_login() {
    let (mut client, vault) = unlocked();
    let login = client
        .save_login(&vault, "https://example.com/login", "", "Ada", "pw", T0)
        .unwrap();
    client
        .passkey_register_hashed(&vault, "example.com", "ada", b"h", T0 + 1)
        .unwrap();
    let all = items(&client, &vault);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["id"], login);
    assert_eq!(all[0]["login"]["passkeys"].as_array().unwrap().len(), 1);
}

#[test]
fn hashed_refusals_have_codes() {
    let (mut client, vault) = unlocked();
    assert_eq!(
        code(client.passkey_register_hashed(&vault, "co.uk", "a", b"h", T0)),
        "invalid_rp_id"
    );
    let created = client
        .passkey_register_hashed(&vault, "example.com", "a", b"h", T0)
        .unwrap();
    let item_id = items(&client, &vault)[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        code(client.passkey_assert_hashed(&vault, &item_id, &[1; 16], "example.com", &HASH)),
        "unknown_item"
    );
    assert_eq!(
        code(client.passkey_assert_hashed(
            &vault,
            &item_id,
            &created.credential_id,
            "other.example",
            &HASH
        )),
        "invalid_rp_id"
    );
    assert_eq!(
        code(client.passkey_assert_hashed(
            &vault,
            &item_id,
            &created.credential_id,
            "example.com",
            &[0; 8]
        )),
        "invalid_passkey_request"
    );
    client.lock();
    assert_eq!(
        code(client.passkey_register_hashed(&vault, "example.com", "a", b"h", T0)),
        "locked"
    );
    assert_eq!(code(client.credential_identities()), "locked");
}

fn identity<'a>(all: &'a [CredentialIdentity], service: &str) -> Vec<&'a CredentialIdentity> {
    all.iter().filter(|i| i.service == service).collect()
}

#[test]
fn identities_hold_domains_and_usernames_only() {
    let (mut client, vault) = unlocked();
    let shop = client
        .save_login(
            &vault,
            "https://shop.example.com/login",
            "",
            "ada",
            "pw1",
            T0,
        )
        .unwrap();
    client
        .save_login(&vault, "https://mail.other.example/", "", "bob", "pw2", T0)
        .unwrap();
    client
        .passkey_register_hashed(&vault, "example.com", "ada", b"h", T0)
        .unwrap();
    let all = client.credential_identities().unwrap();

    // A login with the default (domain) match is suggested on its domain.
    let shop_ids = identity(&all, "example.com");
    let password = shop_ids.iter().find(|i| i.kind == "password").unwrap();
    assert_eq!(password.item_id, shop);
    assert_eq!(password.vault_id, vault);
    assert_eq!(password.user, "ada");
    assert!(password.credential_id.is_empty());
    let passkey = shop_ids.iter().find(|i| i.kind == "passkey").unwrap();
    assert_eq!(passkey.user, "ada");
    assert_eq!(passkey.credential_id.len(), 16);
    assert_eq!(passkey.user_handle, b"h");
    assert_eq!(identity(&all, "other.example").len(), 1);
    // Nothing else leaves the vault: the type has no password or title.
    let text = format!("{all:?}");
    assert!(!text.contains("pw1") && !text.contains("pw2"));
}

#[test]
fn identities_follow_url_matching_and_skip_trash() {
    let (mut client, vault) = unlocked();
    let url = |url: &str, how: UrlMatch| {
        let mut u = LoginUrl::default();
        u.url = url.into();
        u.r#match = how;
        u
    };
    let mut login = Login::default();
    login.username = "h".into();
    login.password = "p".into();
    login.urls = vec![
        url("https://a.example.org/x", UrlMatch::Host),
        url("https://a.example.org/y", UrlMatch::Exact),
        url("^https://b\\.example\\.org", UrlMatch::Regex),
        url("https://never.example.org", UrlMatch::Never),
    ];
    let json = Item::new(ItemKind::Login(login), "Host", T0)
        .to_json_with_id(&ItemId::generate().unwrap())
        .unwrap();
    let id = client.save_item(&vault, json.expose_secret(), T0).unwrap();
    let all = client.credential_identities().unwrap();
    let services: Vec<&str> = all
        .iter()
        .filter(|i| i.item_id == id)
        .map(|i| i.service.as_str())
        .collect();
    // One entry per distinct host; regex and never URLs are not suggested.
    assert_eq!(services, ["a.example.org"]);

    client.set_trashed(&vault, &id, true, T0 + 1).unwrap();
    assert!(client.credential_identities().unwrap().is_empty());
}
