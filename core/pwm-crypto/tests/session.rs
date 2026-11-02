//! The extension's session blob and sealed device secrets (decision D34).

use pwm_crypto::{Aad, AccountKeyset, CryptoError, Envelope, Id, SymmetricKey, random_seed};

const ACCOUNT: Id = [0xA1; 16];
const OTHER: Id = [0xA2; 16];

#[test]
fn a_session_blob_restores_the_same_keyset() {
    let keyset = AccountKeyset::generate().unwrap();
    let (blob, key) = keyset.export_session(&ACCOUNT).unwrap();
    let restored = AccountKeyset::import_session(&blob, &key, &ACCOUNT).unwrap();
    assert_eq!(restored.x25519_public(), keyset.x25519_public());
    assert_eq!(restored.ed25519_public(), keyset.ed25519_public());
    // The same sym_key: it opens what the original sealed.
    let sealed = keyset.seal_device_secret(&ACCOUNT, b"token").unwrap();
    assert_eq!(
        restored
            .open_device_secret(&sealed, &ACCOUNT)
            .unwrap()
            .expose_secret(),
        b"token"
    );
    // Every export uses a fresh key.
    let (_, other_key) = keyset.export_session(&ACCOUNT).unwrap();
    assert_ne!(key.expose_secret(), other_key.expose_secret());
}

#[test]
fn a_session_blob_is_bound_to_its_key_and_account() {
    let keyset = AccountKeyset::generate().unwrap();
    let (blob, key) = keyset.export_session(&ACCOUNT).unwrap();
    let wrong_key = random_seed().unwrap();
    assert!(matches!(
        AccountKeyset::import_session(&blob, &wrong_key, &ACCOUNT),
        Err(CryptoError::DecryptionFailed)
    ));
    assert!(matches!(
        AccountKeyset::import_session(&blob, &key, &OTHER),
        Err(CryptoError::DecryptionFailed)
    ));
    // A keyset sealed for the server (under the AUK, pwm/keyset) is not a
    // session blob, even under the same key bytes.
    let auk = SymmetricKey::from_secret(pwm_crypto::Secret::new(*key.expose_secret()));
    let server_copy = keyset.seal(&auk, &ACCOUNT).unwrap();
    assert!(AccountKeyset::import_session(&server_copy, &key, &ACCOUNT).is_err());
}

#[test]
fn device_secrets_open_only_with_the_keyset_and_account() {
    let keyset = AccountKeyset::generate().unwrap();
    let sealed = keyset
        .seal_device_secret(&ACCOUNT, b"pwmr_refresh")
        .unwrap();
    assert_eq!(
        keyset
            .open_device_secret(&sealed, &ACCOUNT)
            .unwrap()
            .expose_secret(),
        b"pwmr_refresh"
    );
    assert!(keyset.open_device_secret(&sealed, &OTHER).is_err());
    assert!(
        AccountKeyset::generate()
            .unwrap()
            .open_device_secret(&sealed, &ACCOUNT)
            .is_err()
    );
    // A device name (another label under the same sym_key) is not a device secret.
    let name = keyset
        .seal_device_name(&ACCOUNT, &[0xD1; 16], "Laptop")
        .unwrap();
    assert!(keyset.open_device_secret(&name, &ACCOUNT).is_err());
    let _ = Envelope::from_bytes(&sealed.to_bytes()).unwrap();
}

#[test]
fn labels_are_distinct() {
    let session = Aad::session(1, &ACCOUNT);
    let tokens = Aad::device_tokens(1, &ACCOUNT);
    assert!(session.as_bytes().starts_with(b"\x0bpwm/session"));
    assert!(tokens.as_bytes().starts_with(b"\x11pwm/device-tokens"));
    assert_ne!(session.as_bytes(), tokens.as_bytes());
}

#[test]
fn random_seeds_differ() {
    assert_ne!(
        random_seed().unwrap().expose_secret(),
        random_seed().unwrap().expose_secret()
    );
}
