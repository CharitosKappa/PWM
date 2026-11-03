//! Passkeys (D37 §3): RP id rules, registration and assertion bytes, and
//! the request parser's limits. Signatures are checked with `p256` here and
//! independently in `core/test-vectors/verify.mjs`.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use pwm_crypto::Secret;
use pwm_passkey::{
    AAGUID, Caller, Ceremony, CreateRequest, GetRequest, PasskeyError, StoredPasskey, authenticate,
    authenticate_hashed, client_data_json, effective_rp_id, is_secure_origin, register,
    register_hashed, register_hashed_random, register_random,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SEED: [u8; 32] = [0x42; 32];
const CRED: [u8; 16] = [0xC7; 16];
const ORIGIN: &str = "https://login.example.com";

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(text).unwrap()
}

fn create_json(rp_id: Option<&str>, algs: &[i64]) -> String {
    let params: Vec<Value> = algs
        .iter()
        .map(|a| serde_json::json!({"type": "public-key", "alg": a}))
        .collect();
    serde_json::json!({
        "challenge": b64(b"register-challenge-0001"),
        "rp": {"id": rp_id, "name": "Example"},
        "user": {"id": b64(b"user-handle-1"), "name": "ada@example.com", "displayName": "Ada"},
        "pubKeyCredParams": params,
        "excludeCredentials": [{"type": "public-key", "id": b64(&[9; 16])}],
        "authenticatorSelection": {"residentKey": "required", "userVerification": "required"},
        "attestation": "none",
        "timeout": 60000,
    })
    .to_string()
}

fn get_json(rp_id: Option<&str>, allow: &[&[u8]]) -> String {
    let allow: Vec<Value> = allow
        .iter()
        .map(|id| serde_json::json!({"type": "public-key", "id": b64(id)}))
        .collect();
    serde_json::json!({
        "challenge": b64(b"sign-in-challenge-0001"),
        "rpId": rp_id,
        "allowCredentials": allow,
        "userVerification": "preferred",
    })
    .to_string()
}

fn caller() -> Caller<'static> {
    Caller {
        origin: ORIGIN,
        top_origin: None,
    }
}

fn registered() -> (pwm_passkey::Registration, Value) {
    let request = CreateRequest::from_json(&create_json(Some("example.com"), &[-8, -7])).unwrap();
    let registration = register(caller(), &request, &Secret::new(SEED), CRED).unwrap();
    let response: Value = serde_json::from_str(&registration.response_json).unwrap();
    (registration, response)
}

#[test]
fn rp_ids_belong_to_the_page() {
    let ok = |origin: &str, rp: Option<&str>| effective_rp_id(origin, rp);
    assert_eq!(ok(ORIGIN, None).unwrap(), "login.example.com");
    assert_eq!(ok(ORIGIN, Some("example.com")).unwrap(), "example.com");
    assert_eq!(
        ok(ORIGIN, Some("Login.Example.COM.")).unwrap(),
        "login.example.com"
    );
    assert_eq!(ok("http://localhost:3000", None).unwrap(), "localhost");
    assert_eq!(
        ok("https://app.localhost", Some("app.localhost")).unwrap(),
        "app.localhost"
    );
    for (origin, rp, error) in [
        (ORIGIN, Some("evil.example"), PasskeyError::InvalidRpId),
        (ORIGIN, Some("ample.com"), PasskeyError::InvalidRpId),
        (ORIGIN, Some("com"), PasskeyError::InvalidRpId),
        (
            "https://a.github.io",
            Some("github.io"),
            PasskeyError::InvalidRpId,
        ),
        (
            "https://shop.co.uk",
            Some("co.uk"),
            PasskeyError::InvalidRpId,
        ),
        ("https://192.168.1.1", None, PasskeyError::InvalidRpId),
        ("https://[::1]", None, PasskeyError::InvalidRpId),
        (ORIGIN, Some(""), PasskeyError::InvalidRpId),
        (ORIGIN, Some("example.com/x"), PasskeyError::InvalidRpId),
        ("http://example.com", None, PasskeyError::InsecureOrigin),
        ("file:///tmp/x", None, PasskeyError::InsecureOrigin),
        ("not a url", None, PasskeyError::InsecureOrigin),
    ] {
        assert_eq!(ok(origin, rp), Err(error), "{origin} {rp:?}");
    }
    assert!(is_secure_origin("https://example.com"));
    assert!(!is_secure_origin("http://example.com"));
}

#[test]
fn client_data_is_built_from_the_browser_origin() {
    let text = String::from_utf8(client_data_json(Ceremony::Get, b"abc", ORIGIN, None)).unwrap();
    assert_eq!(
        text,
        r#"{"type":"webauthn.get","challenge":"YWJj","origin":"https://login.example.com","crossOrigin":false}"#
    );
    let framed = String::from_utf8(client_data_json(
        Ceremony::Create,
        b"abc",
        "https://a.example.com",
        Some("https://www.example.com"),
    ))
    .unwrap();
    assert_eq!(
        framed,
        r#"{"type":"webauthn.create","challenge":"YWJj","origin":"https://a.example.com","crossOrigin":true,"topOrigin":"https://www.example.com"}"#
    );
    // Odd characters in an origin cannot break out of the JSON string.
    let odd = client_data_json(Ceremony::Get, b"", "https://x.test/\"}", None);
    assert!(serde_json::from_slice::<Value>(&odd).is_ok());
}

#[test]
fn registration_writes_the_expected_bytes() {
    let (registration, response) = registered();
    assert_eq!(response["id"], b64(&CRED));
    assert_eq!(response["rawId"], b64(&CRED));
    assert_eq!(response["type"], "public-key");
    assert_eq!(response["response"]["publicKeyAlgorithm"], -7);

    let auth = unb64(response["response"]["authenticatorData"].as_str().unwrap());
    assert_eq!(&auth[..32], Sha256::digest(b"example.com").as_slice());
    assert_eq!(auth[32], 0x5d, "UP, UV, BE, BS and AT");
    assert_eq!(&auth[33..37], [0, 0, 0, 0], "sign count 0");
    assert_eq!(&auth[37..53], AAGUID);
    assert_eq!(&auth[53..55], [0, 16]);
    assert_eq!(&auth[55..71], CRED);
    let cose = &auth[71..];
    assert_eq!(cose.len(), 77);
    assert_eq!(
        &cose[..10],
        [0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20]
    );
    assert_eq!(&cose[42..45], [0x22, 0x58, 0x20]);

    // The SPKI public key is the COSE key's point.
    let spki = unb64(response["response"]["publicKey"].as_str().unwrap());
    let key = VerifyingKey::from_public_key_der(&spki).unwrap();
    let point = key.to_sec1_point(false);
    assert_eq!(point.x().unwrap().as_slice(), &cose[10..42]);
    assert_eq!(point.y().unwrap().as_slice(), &cose[45..77]);

    // fmt "none", empty attStmt, then authData, deterministic key order.
    let attestation = unb64(response["response"]["attestationObject"].as_str().unwrap());
    let mut expected = vec![0xa3, 0x63];
    expected.extend_from_slice(b"fmt");
    expected.push(0x64);
    expected.extend_from_slice(b"none");
    expected.push(0x67);
    expected.extend_from_slice(b"attStmt");
    expected.extend_from_slice(&[0xa0, 0x68]);
    expected.extend_from_slice(b"authData");
    expected.extend_from_slice(&[0x58, auth.len() as u8]);
    expected.extend_from_slice(&auth);
    assert_eq!(attestation, expected);

    let client = unb64(response["response"]["clientDataJSON"].as_str().unwrap());
    let client: Value = serde_json::from_slice(&client).unwrap();
    assert_eq!(client["type"], "webauthn.create");
    assert_eq!(client["origin"], ORIGIN);
    assert_eq!(client["challenge"], b64(b"register-challenge-0001"));

    let stored = &registration.passkey;
    assert_eq!(stored.credential_id, CRED);
    assert_eq!(stored.rp_id, "example.com");
    assert_eq!(stored.user_handle, b"user-handle-1");
    assert_eq!(stored.user_name, "ada@example.com");
    assert!(!stored.private_key.expose_secret().is_empty());
}

#[test]
fn an_assertion_verifies_with_the_registered_key() {
    let (registration, created) = registered();
    let spki = unb64(created["response"]["publicKey"].as_str().unwrap());
    let key = VerifyingKey::from_public_key_der(&spki).unwrap();

    let request = GetRequest::from_json(&get_json(Some("example.com"), &[&CRED])).unwrap();
    let response: Value =
        serde_json::from_str(&authenticate(caller(), &request, &registration.passkey).unwrap())
            .unwrap();
    let auth = unb64(response["response"]["authenticatorData"].as_str().unwrap());
    assert_eq!(auth.len(), 37);
    assert_eq!(auth[32], 0x1d, "UP, UV, BE and BS");
    let client = unb64(response["response"]["clientDataJSON"].as_str().unwrap());
    let signature = unb64(response["response"]["signature"].as_str().unwrap());
    let mut signed = auth.clone();
    signed.extend_from_slice(&Sha256::digest(&client));
    key.verify(&signed, &Signature::from_der(&signature).unwrap())
        .unwrap();
    assert_eq!(response["response"]["userHandle"], b64(b"user-handle-1"));

    // Discoverable: no allow list works too.
    let any = GetRequest::from_json(&get_json(Some("example.com"), &[])).unwrap();
    assert!(authenticate(caller(), &any, &registration.passkey).is_ok());
}

#[test]
fn assertions_are_refused_for_another_rp_or_an_unlisted_credential() {
    let (registration, _) = registered();
    let other = GetRequest::from_json(&get_json(Some("login.example.com"), &[])).unwrap();
    assert_eq!(
        authenticate(caller(), &other, &registration.passkey),
        Err(PasskeyError::InvalidRpId)
    );
    let unlisted = GetRequest::from_json(&get_json(Some("example.com"), &[&[1; 16]])).unwrap();
    assert_eq!(
        authenticate(caller(), &unlisted, &registration.passkey),
        Err(PasskeyError::InvalidRequest)
    );
    let evil = Caller {
        origin: "https://evil.example",
        top_origin: None,
    };
    let request = GetRequest::from_json(&get_json(Some("example.com"), &[])).unwrap();
    assert_eq!(
        authenticate(evil, &request, &registration.passkey),
        Err(PasskeyError::InvalidRpId)
    );
    let broken = StoredPasskey {
        private_key: Secret::new(vec![1, 2, 3]),
        credential_id: CRED.to_vec(),
        rp_id: "example.com".into(),
        user_handle: vec![1],
        user_name: String::new(),
    };
    assert_eq!(
        authenticate(caller(), &request, &broken),
        Err(PasskeyError::InvalidKey)
    );
}

#[test]
fn only_es256_is_offered() {
    let request = CreateRequest::from_json(&create_json(None, &[-8, -257])).unwrap();
    assert!(!request.es256);
    assert!(matches!(
        register(caller(), &request, &Secret::new(SEED), CRED),
        Err(PasskeyError::UnsupportedAlgorithm)
    ));
    // No list at all means the defaults, which include ES256.
    assert!(
        CreateRequest::from_json(&create_json(None, &[]))
            .unwrap()
            .es256
    );
}

#[test]
fn fresh_registrations_differ() {
    let request = CreateRequest::from_json(&create_json(None, &[-7])).unwrap();
    let a = register_random(caller(), &request).unwrap();
    let b = register_random(caller(), &request).unwrap();
    assert_ne!(a.passkey.credential_id, b.passkey.credential_id);
    assert_ne!(
        a.passkey.private_key.expose_secret(),
        b.passkey.private_key.expose_secret()
    );
}

#[test]
fn the_parser_has_limits() {
    let base: Value = serde_json::from_str(&create_json(None, &[-7])).unwrap();
    let with = |path: &[&str], value: Value| {
        let mut v = base.clone();
        let mut at = &mut v;
        for key in &path[..path.len() - 1] {
            at = &mut at[*key];
        }
        at[path[path.len() - 1]] = value;
        CreateRequest::from_json(&v.to_string())
    };
    assert!(with(&["challenge"], b64(&[1; 1024]).into()).is_ok());
    for (path, value) in [
        (&["challenge"][..], Value::from(b64(&[1; 1025]))),
        (&["challenge"][..], Value::from("")),
        (&["challenge"][..], Value::from("not base64!")),
        (&["user", "id"][..], Value::from(b64(&[1; 65]))),
        (&["user", "name"][..], Value::from("x".repeat(257))),
        (&["rp", "id"][..], Value::from(7)),
        (
            &["excludeCredentials"][..],
            Value::from(vec![
                serde_json::json!({"type": "public-key", "id": "AA"});
                65
            ]),
        ),
    ] {
        assert_eq!(
            with(path, value),
            Err(PasskeyError::InvalidRequest),
            "{path:?}"
        );
    }
    assert_eq!(
        CreateRequest::from_json("{}"),
        Err(PasskeyError::InvalidRequest)
    );
    assert_eq!(
        GetRequest::from_json("[]"),
        Err(PasskeyError::InvalidRequest)
    );
    // Credentials of other types are ignored, not refused.
    let request = GetRequest::from_json(
        &serde_json::json!({"challenge": "AA", "allowCredentials": [{"type": "x", "id": "AA"}]})
            .to_string(),
    )
    .unwrap();
    assert!(request.allow.is_empty());
}

// --- Hashed calls (D47 §6): iOS hands over the RP id and a client data hash ---

const HASH: [u8; 32] = [0x5A; 32];

#[test]
fn a_hashed_registration_writes_the_same_attestation() {
    let (registration, response) = registered();
    let hashed = register_hashed(
        "example.com",
        b"user-handle-1",
        "ada@example.com",
        &Secret::new(SEED),
        CRED,
    )
    .unwrap();
    assert_eq!(
        b64(&hashed.attestation_object),
        response["response"]["attestationObject"].as_str().unwrap()
    );
    let stored = &hashed.passkey;
    assert_eq!(stored.credential_id, CRED);
    assert_eq!(stored.rp_id, "example.com");
    assert_eq!(stored.user_handle, b"user-handle-1");
    assert_eq!(stored.user_name, "ada@example.com");
    assert_eq!(
        stored.private_key.expose_secret(),
        registration.passkey.private_key.expose_secret()
    );
}

#[test]
fn a_hashed_assertion_signs_the_given_hash() {
    let (registration, response) = registered();
    let assertion = authenticate_hashed("example.com", &HASH, &registration.passkey).unwrap();
    let auth = &assertion.authenticator_data;
    assert_eq!(auth.len(), 37);
    assert_eq!(&auth[..32], Sha256::digest(b"example.com").as_slice());
    assert_eq!(auth[32], 0x1d);
    let spki = unb64(response["response"]["publicKey"].as_str().unwrap());
    let key = VerifyingKey::from_public_key_der(&spki).unwrap();
    let mut signed = auth.clone();
    signed.extend_from_slice(&HASH);
    key.verify(&signed, &Signature::from_der(&assertion.signature).unwrap())
        .unwrap();
    // Another hash gives a signature that does not verify for this one.
    let other = authenticate_hashed("example.com", &[1; 32], &registration.passkey).unwrap();
    assert!(
        key.verify(&signed, &Signature::from_der(&other.signature).unwrap())
            .is_err()
    );
}

#[test]
fn hashed_calls_check_the_rp_id_and_the_hash() {
    let (registration, _) = registered();
    let seed = Secret::new(SEED);
    for bad in [
        "",
        "com",
        "co.uk",
        "github.io",
        "exa mple.com",
        "example.com/x",
        "example.com:443",
        "127.0.0.1",
        "[::1]",
    ] {
        assert_eq!(
            register_hashed(bad, b"u", "a", &seed, CRED).err(),
            Some(PasskeyError::InvalidRpId),
            "{bad}"
        );
    }
    let upper = register_hashed("Login.Example.COM.", b"u", "a", &seed, CRED).unwrap();
    assert_eq!(upper.passkey.rp_id, "login.example.com");
    assert!(register_hashed("localhost", b"u", "a", &seed, CRED).is_ok());
    assert_eq!(
        authenticate_hashed("other.example", &HASH, &registration.passkey).err(),
        Some(PasskeyError::InvalidRpId)
    );
    assert_eq!(
        authenticate_hashed("example.com", &[0; 31], &registration.passkey).err(),
        Some(PasskeyError::InvalidRequest)
    );
    let broken = StoredPasskey {
        private_key: Secret::new(vec![1, 2, 3]),
        ..stored_copy(&registration.passkey)
    };
    assert_eq!(
        authenticate_hashed("example.com", &HASH, &broken).err(),
        Some(PasskeyError::InvalidKey)
    );
}

#[test]
fn fresh_hashed_registrations_differ() {
    let a = register_hashed_random("example.com", b"u", "a").unwrap();
    let b = register_hashed_random("example.com", b"u", "a").unwrap();
    assert_ne!(a.passkey.credential_id, b.passkey.credential_id);
    assert_ne!(a.attestation_object, b.attestation_object);
}

fn stored_copy(p: &StoredPasskey) -> StoredPasskey {
    StoredPasskey {
        credential_id: p.credential_id.clone(),
        rp_id: p.rp_id.clone(),
        user_handle: p.user_handle.clone(),
        user_name: p.user_name.clone(),
        private_key: Secret::new(p.private_key.expose_secret().clone()),
    }
}
