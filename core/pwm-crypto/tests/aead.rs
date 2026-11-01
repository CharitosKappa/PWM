//! Public API tests for the v1 envelope (D10) and AAD layout (D11).

use proptest::prelude::*;
use pwm_crypto::{Aad, CryptoError, Envelope, Id, SymmetricKey, open, seal};

const VAULT: Id = [0x11; 16];
const ITEM: Id = [0x22; 16];
const ACCOUNT: Id = [0x33; 16];

fn key() -> SymmetricKey {
    SymmetricKey::generate().expect("rng")
}

fn item_aad() -> Aad {
    Aad::item(1, &VAULT, &ITEM, 7)
}

#[test]
fn round_trip() {
    let k = key();
    let env = seal(&k, b"correct horse", &item_aad()).unwrap();
    let pt = open(&k, &env, &item_aad()).unwrap();
    assert_eq!(pt.expose_secret().as_slice(), b"correct horse");
}

#[test]
fn round_trip_empty_plaintext() {
    let k = key();
    let env = seal(&k, b"", &item_aad()).unwrap();
    assert_eq!(env.to_bytes().len(), Envelope::MIN_LEN);
    let pt = open(&k, &env, &item_aad()).unwrap();
    assert!(pt.expose_secret().is_empty());
}

#[test]
fn wire_layout() {
    let k = key();
    let env = seal(&k, b"abc", &item_aad()).unwrap();
    let bytes = env.to_bytes();
    assert_eq!(bytes[0], Envelope::VERSION);
    assert_eq!(Envelope::VERSION, 1);
    assert_eq!(bytes.len(), 1 + 24 + 3 + 16);
    assert_eq!(Envelope::MIN_LEN, 41);
    let decoded = Envelope::from_bytes(&bytes).unwrap();
    assert_eq!(decoded.to_bytes(), bytes);
}

#[test]
fn fresh_nonce_per_seal() {
    let k = key();
    let a = seal(&k, b"same", &item_aad()).unwrap().to_bytes();
    let b = seal(&k, b"same", &item_aad()).unwrap().to_bytes();
    assert_ne!(a[1..25], b[1..25]);
    assert_ne!(a, b);
}

#[test]
fn wrong_key_fails() {
    let env = seal(&key(), b"secret", &item_aad()).unwrap();
    assert_eq!(
        open(&key(), &env, &item_aad()).unwrap_err(),
        CryptoError::DecryptionFailed
    );
}

#[test]
fn wrong_aad_fails() {
    let k = key();
    let env = seal(&k, b"secret", &item_aad()).unwrap();
    let others = [
        Aad::item(2, &VAULT, &ITEM, 7),
        Aad::item(1, &ITEM, &ITEM, 7),
        Aad::item(1, &VAULT, &VAULT, 7),
        Aad::item(1, &VAULT, &ITEM, 8),
        Aad::vault_key(1, &VAULT),
        Aad::keyset(1, &ACCOUNT),
    ];
    for aad in &others {
        assert_eq!(
            open(&k, &env, aad).unwrap_err(),
            CryptoError::DecryptionFailed
        );
    }
}

#[test]
fn every_bit_flip_is_rejected() {
    let k = key();
    let bytes = seal(&k, b"tamper me", &item_aad()).unwrap().to_bytes();
    for i in 0..bytes.len() * 8 {
        let mut t = bytes.clone();
        t[i / 8] ^= 1 << (i % 8);
        let result = Envelope::from_bytes(&t).and_then(|e| open(&k, &e, &item_aad()));
        assert!(result.is_err(), "bit {i} flip accepted");
    }
}

#[test]
fn every_truncation_is_rejected() {
    let k = key();
    let bytes = seal(&k, b"tamper me", &item_aad()).unwrap().to_bytes();
    for len in 0..bytes.len() {
        let result = Envelope::from_bytes(&bytes[..len]).and_then(|e| open(&k, &e, &item_aad()));
        assert!(result.is_err(), "truncation to {len} accepted");
    }
}

#[test]
fn decode_rejects_unknown_version_and_short_input() {
    let mut bytes = vec![1u8; Envelope::MIN_LEN];
    for v in [0u8, 2, 0x80, 0xFF] {
        bytes[0] = v;
        assert_eq!(
            Envelope::from_bytes(&bytes).unwrap_err(),
            CryptoError::UnsupportedVersion(v)
        );
    }
    assert_eq!(
        Envelope::from_bytes(&[1u8; 40]).unwrap_err(),
        CryptoError::Truncated
    );
    assert_eq!(
        Envelope::from_bytes(&[]).unwrap_err(),
        CryptoError::Truncated
    );
}

#[test]
fn aad_item_layout() {
    let aad = Aad::item(3, &VAULT, &ITEM, 0x0102_0304_0506_0708);
    let mut expected = vec![8u8];
    expected.extend_from_slice(b"pwm/item");
    expected.push(3);
    expected.extend_from_slice(&VAULT);
    expected.extend_from_slice(&ITEM);
    expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(aad.as_bytes(), expected.as_slice());
}

#[test]
fn aad_vault_key_layout() {
    let aad = Aad::vault_key(1, &VAULT);
    let mut expected = vec![13u8];
    expected.extend_from_slice(b"pwm/vault-key");
    expected.push(1);
    expected.extend_from_slice(&VAULT);
    assert_eq!(aad.as_bytes(), expected.as_slice());
}

#[test]
fn aad_keyset_layout() {
    let aad = Aad::keyset(1, &ACCOUNT);
    let mut expected = vec![10u8];
    expected.extend_from_slice(b"pwm/keyset");
    expected.push(1);
    expected.extend_from_slice(&ACCOUNT);
    assert_eq!(aad.as_bytes(), expected.as_slice());
}

#[test]
fn key_debug_is_redacted() {
    let k = SymmetricKey::from_secret(pwm_crypto::Secret::new([0x42; 32]));
    let out = format!("{k:?}");
    assert!(out.contains("REDACTED"), "{out}");
    assert!(!out.contains("42"), "{out}");
}

proptest! {
    #[test]
    fn prop_round_trip(pt in proptest::collection::vec(any::<u8>(), 0..512),
                       version: u8, vault: Id, item: Id, revision: u64) {
        let k = key();
        let aad = Aad::item(version, &vault, &item, revision);
        let bytes = seal(&k, &pt, &aad).unwrap().to_bytes();
        let env = Envelope::from_bytes(&bytes).unwrap();
        let out = open(&k, &env, &aad).unwrap();
        prop_assert_eq!(out.expose_secret(), &pt);
    }

    #[test]
    fn prop_decode_never_panics_and_reencodes(bytes in proptest::collection::vec(any::<u8>(), 0..128)) {
        if let Ok(env) = Envelope::from_bytes(&bytes) {
            prop_assert_eq!(env.to_bytes(), bytes);
        }
    }
}
