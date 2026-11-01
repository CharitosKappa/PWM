//! Checks every JSON vector in `core/test-vectors/` (run with
//! `just test-vectors`). Each ciphertext vector is also tamper-tested: every
//! single-bit flip must be rejected.

use pwm_crypto::{
    Aad, AccountKeyset, Envelope, Id, KdfParams, Secret, SecretKey, SignatureContext, SymmetricKey,
    VaultShare, derive_account_keys, derive_password_key, hkdf_sha256, normalize_password, open,
    seal, unlock_account, verify_signature,
};
use serde_json::Value;

fn load(json: &str) -> Vec<Value> {
    let doc: Value = serde_json::from_str(json).unwrap();
    assert_eq!(doc["format_version"], 1);
    let vectors = doc["vectors"].as_array().unwrap().clone();
    assert!(!vectors.is_empty());
    vectors
}

fn bytes(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap()).unwrap()
}

fn arr<const N: usize>(v: &Value) -> [u8; N] {
    bytes(v).try_into().unwrap()
}

fn text(v: &Value) -> String {
    String::from_utf8(bytes(v)).unwrap()
}

fn num(v: &Value) -> u64 {
    v.as_u64().unwrap()
}

/// Revisions are decimal strings: JSON numbers above 2^53 lose precision in
/// JavaScript.
fn revision(v: &Value) -> u64 {
    v.as_str().unwrap().parse().unwrap()
}

fn key(v: &Value) -> SymmetricKey {
    SymmetricKey::from_secret(Secret::new(arr(v)))
}

fn params(v: &Value) -> KdfParams {
    KdfParams {
        memory_kib: num(&v["memory_kib"]) as u32,
        iterations: num(&v["iterations"]) as u32,
        parallelism: num(&v["parallelism"]) as u32,
    }
}

fn keyset(v: &Value) -> AccountKeyset {
    let sealed = Envelope::from_bytes(&bytes(&v["sealed_keyset"])).unwrap();
    AccountKeyset::open(&sealed, &key(&v["auk"]), &arr(&v["account_id"])).unwrap()
}

/// Every single-bit flip of `data` must make `accept` fail.
fn assert_tamper_evident(data: &[u8], accept: impl Fn(&[u8]) -> bool) {
    assert!(accept(data), "untampered input must be accepted");
    for i in 0..data.len() * 8 {
        let mut t = data.to_vec();
        t[i / 8] ^= 1 << (i % 8);
        assert!(!accept(&t), "bit {i} flip accepted");
    }
}

#[test]
fn vectors_aad() {
    for v in load(include_str!("../../test-vectors/aad.json")) {
        let version = num(&v["version"]) as u8;
        let aad = match v["context"].as_str().unwrap() {
            "item" => Aad::item(
                version,
                &arr(&v["vault_id"]),
                &arr(&v["item_id"]),
                revision(&v["revision"]),
            ),
            "vault-key" => Aad::vault_key(version, &arr(&v["vault_id"])),
            "keyset" => Aad::keyset(version, &arr(&v["account_id"])),
            "share" => Aad::share(
                version,
                &arr(&v["vault_id"]),
                &arr(&v["sender_ed25519_pub"]),
                &arr(&v["recipient_x25519_pub"]),
            ),
            other => panic!("unknown AAD context {other}"),
        };
        assert_eq!(aad.as_bytes(), bytes(&v["aad"]).as_slice());
    }
}

#[test]
fn vectors_aead() {
    for v in load(include_str!("../../test-vectors/aead.json")) {
        let aad = Aad::item(
            num(&v["version"]) as u8,
            &arr(&v["vault_id"]),
            &arr(&v["item_id"]),
            revision(&v["revision"]),
        );
        assert_eq!(aad.as_bytes(), bytes(&v["aad"]).as_slice());
        let k = key(&v["key"]);
        let plaintext = bytes(&v["plaintext"]);
        assert_tamper_evident(&bytes(&v["envelope"]), |b| {
            Envelope::from_bytes(b)
                .and_then(|env| open(&k, &env, &aad))
                .is_ok_and(|pt| *pt.expose_secret() == plaintext)
        });
    }
}

#[test]
fn vectors_hkdf() {
    for v in load(include_str!("../../test-vectors/hkdf.json")) {
        let okm = hkdf_sha256(&bytes(&v["ikm"]), &bytes(&v["salt"]), &bytes(&v["info"]));
        assert_eq!(okm.expose_secret().as_slice(), bytes(&v["okm"]));
    }
}

#[test]
fn vectors_kdf() {
    for v in load(include_str!("../../test-vectors/kdf.json")) {
        let out = derive_password_key(
            &bytes(&v["password"]),
            &arr(&v["salt"]),
            &params(&v["params"]),
        )
        .unwrap();
        assert_eq!(out.expose_secret().as_slice(), bytes(&v["pw_key"]));
    }
}

#[test]
fn vectors_password() {
    for v in load(include_str!("../../test-vectors/password.json")) {
        let input = text(&v["input"]);
        match &v["normalized"] {
            Value::Null => assert!(normalize_password(&input).is_err(), "{input:?}"),
            expected => assert_eq!(
                normalize_password(&input).unwrap().expose_secret(),
                &text(expected)
            ),
        }
    }
}

#[test]
fn vectors_secret_key() {
    for v in load(include_str!("../../test-vectors/secret_key.json")) {
        let expected = v["text"].as_str().unwrap();
        let from_bytes = SecretKey::from_bytes(Secret::new(arr(&v["bytes"])));
        assert_eq!(from_bytes.to_text().expose_secret(), expected);
        let parsed = SecretKey::parse(expected).unwrap();
        assert_eq!(parsed.to_text().expose_secret(), expected);
    }
}

#[test]
fn vectors_account() {
    for v in load(include_str!("../../test-vectors/account.json")) {
        let password = text(&v["password"]);
        let secret_key = SecretKey::parse(v["secret_key"].as_str().unwrap()).unwrap();
        let account: Id = arr(&v["account_id"]);
        let salt = arr(&v["salt"]);
        let p = params(&v["params"]);

        let normalized = normalize_password(&password).unwrap();
        let pw_key = derive_password_key(normalized.expose_secret().as_bytes(), &salt, &p).unwrap();
        assert_eq!(pw_key.expose_secret().as_slice(), bytes(&v["pw_key"]));
        let sk_bytes: [u8; 16] = arr(&v["secret_key_bytes"]);
        assert_eq!(
            SecretKey::from_bytes(Secret::new(sk_bytes))
                .to_text()
                .expose_secret(),
            v["secret_key"].as_str().unwrap()
        );
        let sk_key = hkdf_sha256(&sk_bytes, &account, b"pwm/sk/v1");
        assert_eq!(sk_key.expose_secret().as_slice(), bytes(&v["sk_key"]));
        let mixed: Vec<u8> = pw_key
            .expose_secret()
            .iter()
            .zip(sk_key.expose_secret())
            .map(|(a, b)| a ^ b)
            .collect();
        assert_eq!(
            hkdf_sha256(&mixed, &account, b"pwm/auk/v1")
                .expose_secret()
                .as_slice(),
            bytes(&v["auk"])
        );

        let keys = derive_account_keys(&password, &secret_key, &account, &salt, &p).unwrap();
        assert_eq!(
            keys.auth_secret().expose_secret().as_slice(),
            bytes(&v["auth_secret"])
        );

        let sealed = bytes(&v["sealed_keyset"]);
        // The recorded AUK and the derived AUK both open the sealed keyset.
        let recorded_auk = key(&v["auk"]);
        let ks = keyset(&v);
        assert_eq!(ks.x25519_public().as_slice(), bytes(&v["x25519_public"]));
        assert_eq!(ks.ed25519_public().as_slice(), bytes(&v["ed25519_public"]));
        let unlocked = unlock_account(
            &password,
            &secret_key,
            &account,
            &salt,
            &p,
            &Envelope::from_bytes(&sealed).unwrap(),
        )
        .unwrap();
        assert_eq!(unlocked.ed25519_public(), ks.ed25519_public());

        assert_tamper_evident(&sealed, |b| {
            Envelope::from_bytes(b)
                .and_then(|env| AccountKeyset::open(&env, &recorded_auk, &account))
                .is_ok()
        });
    }
}

#[test]
fn vectors_vault_key() {
    for v in load(include_str!("../../test-vectors/vault_key.json")) {
        let ks = keyset(&v);
        let vault: Id = arr(&v["vault_id"]);
        let probe_aad = Aad::item(
            1,
            &vault,
            &arr(&v["probe_item_id"]),
            revision(&v["probe_revision"]),
        );
        let probe = Envelope::from_bytes(&bytes(&v["probe_envelope"])).unwrap();
        let probe_plaintext = bytes(&v["probe_plaintext"]);
        assert_tamper_evident(&bytes(&v["wrapped"]), |b| {
            Envelope::from_bytes(b)
                .and_then(|env| ks.unwrap_vault_key(&env, &vault))
                .and_then(|vk| open(&vk, &probe, &probe_aad))
                .is_ok_and(|pt| *pt.expose_secret() == probe_plaintext)
        });
    }
}

#[test]
fn vectors_signature() {
    for v in load(include_str!("../../test-vectors/signature.json")) {
        assert_eq!(v["context"], "share");
        let ks = keyset(&v);
        let public: [u8; 32] = arr(&v["ed25519_public"]);
        assert_eq!(ks.ed25519_public(), public);
        let message = bytes(&v["message"]);
        let signature: [u8; 64] = arr(&v["signature"]);
        // Ed25519 is deterministic: signing again gives the same bytes.
        assert_eq!(ks.sign(SignatureContext::Share, &message), signature);
        assert_tamper_evident(&signature, |b| {
            verify_signature(
                &public,
                SignatureContext::Share,
                &message,
                b.try_into().unwrap(),
            )
            .is_ok()
        });
    }
}

#[test]
fn vectors_share() {
    for v in load(include_str!("../../test-vectors/share.json")) {
        let sender = keyset(&v["sender"]);
        let recipient = keyset(&v["recipient"]);
        let sender_pub: [u8; 32] = arr(&v["sender"]["ed25519_public"]);
        assert_eq!(sender.ed25519_public(), sender_pub);
        assert_eq!(
            recipient.x25519_public().as_slice(),
            bytes(&v["recipient"]["x25519_public"])
        );
        let vault: Id = arr(&v["vault_id"]);
        // Prove the accepted key equals the recorded vault key.
        let probe_aad = Aad::item(1, &vault, &[0; 16], 0);
        let probe = seal(&key(&v["vault_key"]), b"probe", &probe_aad).unwrap();
        assert_tamper_evident(&bytes(&v["share"]), |b| {
            VaultShare::from_bytes(b)
                .and_then(|s| recipient.accept_vault_share(&s, &vault, &sender_pub))
                .and_then(|vk| open(&vk, &probe, &probe_aad))
                .is_ok_and(|pt| pt.expose_secret() == b"probe")
        });
    }
}
