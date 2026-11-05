//! Passkeys for Android's Credential Manager (D61): the client data an app
//! gets, a browser's client data hash, and the Digital Asset Links check.
//! Signatures are checked with `p256` here and in `verify.mjs`.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use pwm_crypto::Secret;
use pwm_passkey::{
    AndroidCaller, CLIENT_DATA_PLACEHOLDER, Ceremony, CreateRequest, GetRequest, MAX_ASSET_LINKS,
    PasskeyError, apk_key_hash_origin, app_client_data_json, asset_links_includes, asset_links_url,
    authenticate_android, read_asset_links, register_android,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SEED: [u8; 32] = [0x42; 32];
const CRED: [u8; 16] = [0xC7; 16];
const PACKAGE: &str = "com.example.app";
const CERT: [u8; 32] = [0xAB; 32];

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(text).unwrap()
}

fn create_json(rp_id: Option<&str>) -> String {
    json!({
        "challenge": b64(b"register-challenge-0001"),
        "rp": {"id": rp_id, "name": "Example"},
        "user": {"id": b64(b"user-handle-1"), "name": "ada@example.com", "displayName": "Ada"},
        "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
    })
    .to_string()
}

fn get_json(rp_id: Option<&str>) -> String {
    json!({"challenge": b64(b"sign-in-challenge-0001"), "rpId": rp_id}).to_string()
}

fn app() -> AndroidCaller<'static> {
    AndroidCaller::App {
        package: PACKAGE,
        cert_sha256: &CERT,
    }
}

fn fingerprint(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn statement(relation: &str, package: &str, cert: &[u8]) -> Value {
    json!({
        "relation": [relation],
        "target": {
            "namespace": "android_app",
            "package_name": package,
            "sha256_cert_fingerprints": [fingerprint(cert)],
        },
    })
}

#[test]
fn the_apps_origin_is_its_certificates_hash() {
    assert_eq!(
        apk_key_hash_origin(&CERT).unwrap(),
        format!("android:apk-key-hash:{}", b64(&CERT))
    );
    // base64url without padding: no '=', '+' or '/'.
    let origin = apk_key_hash_origin(&[0xFB; 32]).unwrap();
    assert!(!origin.contains(['=', '+', '/']), "{origin}");
    assert_eq!(
        apk_key_hash_origin(&[1; 31]).unwrap_err(),
        PasskeyError::InvalidRequest
    );
}

#[test]
fn an_apps_client_data_names_its_package() {
    let origin = apk_key_hash_origin(&CERT).unwrap();
    let data = app_client_data_json(Ceremony::Get, b"abc", &origin, PACKAGE);
    // The fields and order of Google's own provider: no crossOrigin.
    assert_eq!(
        String::from_utf8(data).unwrap(),
        format!(
            r#"{{"type":"webauthn.get","challenge":"YWJj","origin":"{origin}","androidPackageName":"com.example.app"}}"#
        )
    );
}

#[test]
fn an_app_registers_and_signs_in_with_its_own_client_data() {
    let request = CreateRequest::from_json(&create_json(Some("example.com"))).unwrap();
    let registration =
        register_android(app(), &request, &Secret::new(SEED), CRED).expect("registers");
    assert_eq!(registration.passkey.rp_id, "example.com");
    let response: Value = serde_json::from_str(&registration.response_json).unwrap();
    let client_data: Value = serde_json::from_slice(&unb64(
        response["response"]["clientDataJSON"].as_str().unwrap(),
    ))
    .unwrap();
    assert_eq!(client_data["type"], "webauthn.create");
    assert_eq!(client_data["origin"], apk_key_hash_origin(&CERT).unwrap());
    assert_eq!(client_data["androidPackageName"], PACKAGE);
    assert_eq!(client_data["challenge"], b64(b"register-challenge-0001"));

    let get = GetRequest::from_json(&get_json(Some("example.com"))).unwrap();
    let assertion: Value =
        serde_json::from_str(&authenticate_android(app(), &get, &registration.passkey).unwrap())
            .unwrap();
    let client_data = unb64(assertion["response"]["clientDataJSON"].as_str().unwrap());
    let auth_data = unb64(assertion["response"]["authenticatorData"].as_str().unwrap());
    let key = VerifyingKey::from_public_key_der(&unb64(
        response["response"]["publicKey"].as_str().unwrap(),
    ))
    .unwrap();
    let mut signed = auth_data.clone();
    signed.extend_from_slice(&Sha256::digest(&client_data));
    let signature =
        Signature::from_der(&unb64(assertion["response"]["signature"].as_str().unwrap())).unwrap();
    key.verify(&signed, &signature).expect("signature verifies");
    assert_eq!(&auth_data[..32], Sha256::digest(b"example.com").as_slice());
}

#[test]
fn an_app_must_name_its_rp_id() {
    let request = CreateRequest::from_json(&create_json(None)).unwrap();
    assert_eq!(
        register_android(app(), &request, &Secret::new(SEED), CRED).err(),
        Some(PasskeyError::InvalidRequest)
    );
    // Never a public suffix, as for any RP id without a page.
    let request = CreateRequest::from_json(&create_json(Some("co.uk"))).unwrap();
    assert_eq!(
        register_android(app(), &request, &Secret::new(SEED), CRED).err(),
        Some(PasskeyError::InvalidRpId)
    );
    // Found by the fuzz target: under the request's 256 characters as
    // typed, over DNS's 253 in ASCII form, so its passkey could never sign in.
    let long = "Pbc.xn505.39Wan\u{2e2}X.xn50.X50.Xc.xn505.39Wan\u{2e2}X.xn50.X50.xn50.X50.X39Wan\u{2e2}X.3x.X50.X3a.3350.Xc.xn505.39Wan\u{2e2}X.xn50.X50.xn50.X50.X39Wan\u{2e2}X.3x.X50.X3a.338Wan\u{2e2}X.xnX05.39Wan\u{2e2}X.xn50.X50.X39WaN\u{2e2}X.xnX05.39Wan\u{2e2}X.xn50.X50.X39Wan\u{1a2}X.xn50.X50.X39Wan\u{2e2}X.3x.X50.X3a.3x.X50.X3a";
    let request = CreateRequest::from_json(&create_json(Some(long))).unwrap();
    assert_eq!(
        register_android(app(), &request, &Secret::new(SEED), CRED).err(),
        Some(PasskeyError::InvalidRpId)
    );
    let bad = AndroidCaller::App {
        package: "not a package",
        cert_sha256: &CERT,
    };
    let request = CreateRequest::from_json(&create_json(Some("example.com"))).unwrap();
    assert_eq!(
        register_android(bad, &request, &Secret::new(SEED), CRED).err(),
        Some(PasskeyError::InvalidRequest)
    );
}

#[test]
fn a_browser_hands_over_the_hash_and_its_origin_is_checked() {
    let hash: [u8; 32] = Sha256::digest(b"chrome-built-client-data").into();
    let browser = AndroidCaller::Browser {
        origin: "https://login.example.com",
        client_data_hash: &hash,
    };
    let request = CreateRequest::from_json(&create_json(Some("example.com"))).unwrap();
    let registration =
        register_android(browser, &request, &Secret::new(SEED), CRED).expect("registers");
    let response: Value = serde_json::from_str(&registration.response_json).unwrap();
    // The browser puts its own client data in; ours is a placeholder.
    assert_eq!(
        response["response"]["clientDataJSON"],
        CLIENT_DATA_PLACEHOLDER
    );
    assert_eq!(unb64(CLIENT_DATA_PLACEHOLDER), b"{}");

    let get = GetRequest::from_json(&get_json(Some("example.com"))).unwrap();
    let assertion: Value =
        serde_json::from_str(&authenticate_android(browser, &get, &registration.passkey).unwrap())
            .unwrap();
    assert_eq!(
        assertion["response"]["clientDataJSON"],
        CLIENT_DATA_PLACEHOLDER
    );
    let auth_data = unb64(assertion["response"]["authenticatorData"].as_str().unwrap());
    let key = VerifyingKey::from_public_key_der(&unb64(
        response["response"]["publicKey"].as_str().unwrap(),
    ))
    .unwrap();
    let mut signed = auth_data;
    signed.extend_from_slice(&hash);
    let signature =
        Signature::from_der(&unb64(assertion["response"]["signature"].as_str().unwrap())).unwrap();
    key.verify(&signed, &signature)
        .expect("signed over the browser's hash");

    // Another site's RP id, an insecure origin, a short hash.
    let other = CreateRequest::from_json(&create_json(Some("other.example"))).unwrap();
    assert_eq!(
        register_android(browser, &other, &Secret::new(SEED), CRED).err(),
        Some(PasskeyError::InvalidRpId)
    );
    let http = AndroidCaller::Browser {
        origin: "http://login.example.com",
        client_data_hash: &hash,
    };
    assert_eq!(
        register_android(http, &request, &Secret::new(SEED), CRED).err(),
        Some(PasskeyError::InsecureOrigin)
    );
    let short = AndroidCaller::Browser {
        origin: "https://login.example.com",
        client_data_hash: &hash[..31],
    };
    assert_eq!(
        authenticate_android(short, &get, &registration.passkey).err(),
        Some(PasskeyError::InvalidRequest)
    );
}

#[test]
fn a_passkey_signs_only_for_its_own_rp_id() {
    let request = CreateRequest::from_json(&create_json(Some("example.com"))).unwrap();
    let registration = register_android(app(), &request, &Secret::new(SEED), CRED).unwrap();
    let get = GetRequest::from_json(&get_json(Some("other.example"))).unwrap();
    assert_eq!(
        authenticate_android(app(), &get, &registration.passkey).err(),
        Some(PasskeyError::InvalidRpId)
    );
}

#[test]
fn the_asset_links_address_is_the_rp_ids_well_known_file() {
    assert_eq!(
        asset_links_url("Example.com").unwrap(),
        "https://example.com/.well-known/assetlinks.json"
    );
    assert_eq!(
        asset_links_url("co.uk").unwrap_err(),
        PasskeyError::InvalidRpId
    );
    assert_eq!(
        asset_links_url("example.com/evil").unwrap_err(),
        PasskeyError::InvalidRpId
    );
}

#[test]
fn asset_links_list_the_app_by_package_and_certificate() {
    for relation in [
        "delegate_permission/common.get_login_creds",
        "delegate_permission/common.handle_all_urls",
    ] {
        let file = json!([statement(relation, PACKAGE, &CERT)]).to_string();
        assert!(
            read_asset_links(&file, PACKAGE, &CERT).unwrap().linked,
            "{relation}"
        );
    }
    // Lowercase hex, as some sites write it.
    let mut lower = statement("delegate_permission/common.handle_all_urls", PACKAGE, &CERT);
    lower["target"]["sha256_cert_fingerprints"] = json!([fingerprint(&CERT).to_lowercase()]);
    assert!(
        read_asset_links(&json!([lower]).to_string(), PACKAGE, &CERT)
            .unwrap()
            .linked
    );
    // Several certificates and relations in one statement.
    let several = json!([{
        "relation": ["delegate_permission/common.use_as_origin", "delegate_permission/common.get_login_creds"],
        "target": {"namespace": "android_app", "package_name": PACKAGE,
                   "sha256_cert_fingerprints": [fingerprint(&[1; 32]), fingerprint(&CERT)]},
    }]);
    assert!(
        read_asset_links(&several.to_string(), PACKAGE, &CERT)
            .unwrap()
            .linked
    );
}

#[test]
fn asset_links_for_anything_else_do_not_count() {
    let relation = "delegate_permission/common.get_login_creds";
    let cases = [
        json!([statement(relation, "com.example.other", &CERT)]),
        json!([statement(relation, PACKAGE, &[0xCD; 32])]),
        json!([statement(
            "delegate_permission/common.use_as_origin",
            PACKAGE,
            &CERT
        )]),
        json!([{"relation": [relation], "target": {"namespace": "web", "site": "https://example.com"}}]),
        json!([{"relation": [relation], "target": {"namespace": "android_app", "package_name": PACKAGE,
                 "sha256_cert_fingerprints": ["AB:AB"]}}]),
        json!([]),
    ];
    for file in cases {
        assert!(
            !read_asset_links(&file.to_string(), PACKAGE, &CERT)
                .unwrap()
                .linked,
            "{file}"
        );
    }
}

#[test]
fn a_malformed_or_large_file_is_refused() {
    for text in ["", "{}", "not json", r#"{"relation": []}"#, "[1, 2]"] {
        assert_eq!(
            read_asset_links(text, PACKAGE, &CERT).unwrap_err(),
            PasskeyError::InvalidAssetLinks,
            "{text}"
        );
    }
    let mut big = json!([statement(
        "delegate_permission/common.get_login_creds",
        PACKAGE,
        &CERT
    )])
    .to_string();
    big.push_str(&" ".repeat(MAX_ASSET_LINKS));
    assert_eq!(
        read_asset_links(&big, PACKAGE, &CERT).unwrap_err(),
        PasskeyError::InvalidAssetLinks
    );
}

#[test]
fn includes_are_listed_for_the_platform_to_fetch() {
    let file = json!([
        {"include": "https://other.example/.well-known/assetlinks.json"},
        {"include": "http://plain.example/.well-known/assetlinks.json"},
        {"include": "https://other.example/.well-known/assetlinks.json"},
        {"include": "https://10.0.0.1/assetlinks.json"},
        statement("delegate_permission/common.get_login_creds", "com.example.other", &CERT),
    ])
    .to_string();
    let read = read_asset_links(&file, PACKAGE, &CERT).unwrap();
    assert!(!read.linked);
    // HTTPS on a domain only, each once.
    assert_eq!(
        read.includes,
        vec!["https://other.example/.well-known/assetlinks.json"]
    );
    assert_eq!(asset_links_includes(&file).unwrap(), read.includes);
    // At most 10.
    let many: Vec<Value> = (0..20)
        .map(|i| json!({"include": format!("https://s{i}.example/.well-known/assetlinks.json")}))
        .collect();
    assert_eq!(
        asset_links_includes(&Value::from(many).to_string())
            .unwrap()
            .len(),
        10
    );
}
