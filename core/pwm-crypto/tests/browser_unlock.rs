//! The browser unlock blob (decision D48 §5): the extension's opened keyset
//! sealed under a random key that the Mac's helper keeps behind Touch ID.

use pwm_crypto::{Aad, AccountKeyset, CryptoError, Id, random_seed};

const ACCOUNT: Id = [0xA1; 16];
const OTHER_ACCOUNT: Id = [0xA2; 16];
const DEVICE: Id = [0xD1; 16];
const OTHER_DEVICE: Id = [0xD2; 16];

#[test]
fn the_blob_restores_the_same_keyset() {
    let keyset = AccountKeyset::generate().unwrap();
    let (blob, key) = keyset.export_browser_unlock(&ACCOUNT, &DEVICE).unwrap();
    let restored = AccountKeyset::import_browser_unlock(&blob, &key, &ACCOUNT, &DEVICE).unwrap();
    assert_eq!(restored.x25519_public(), keyset.x25519_public());
    assert_eq!(restored.ed25519_public(), keyset.ed25519_public());
    let (_, other_key) = keyset.export_browser_unlock(&ACCOUNT, &DEVICE).unwrap();
    assert_ne!(key.expose_secret(), other_key.expose_secret());
}

#[test]
fn the_blob_is_bound_to_its_key_account_and_device() {
    let keyset = AccountKeyset::generate().unwrap();
    let (blob, key) = keyset.export_browser_unlock(&ACCOUNT, &DEVICE).unwrap();
    let wrong_key = random_seed().unwrap();
    for (key, account, device) in [
        (&wrong_key, &ACCOUNT, &DEVICE),
        (&key, &OTHER_ACCOUNT, &DEVICE),
        (&key, &ACCOUNT, &OTHER_DEVICE),
    ] {
        assert!(matches!(
            AccountKeyset::import_browser_unlock(&blob, key, account, device),
            Err(CryptoError::DecryptionFailed)
        ));
    }
}

#[test]
fn it_opens_neither_the_face_id_nor_the_session_blob() {
    let keyset = AccountKeyset::generate().unwrap();
    let (blob, key) = keyset.export_browser_unlock(&ACCOUNT, &DEVICE).unwrap();
    assert!(AccountKeyset::import_biometric_unlock(&blob, &key, &ACCOUNT, &DEVICE).is_err());
    assert!(AccountKeyset::import_session(&blob, &key, &ACCOUNT).is_err());
    let (face, face_key) = keyset.export_biometric_unlock(&ACCOUNT, &DEVICE).unwrap();
    assert!(AccountKeyset::import_browser_unlock(&face, &face_key, &ACCOUNT, &DEVICE).is_err());
}

#[test]
fn its_label_is_its_own() {
    let aad = Aad::browser_unlock(1, &ACCOUNT, &DEVICE);
    let bytes = aad.as_bytes();
    let label = b"pwm/browser-unlock";
    assert_eq!(bytes[0] as usize, label.len());
    assert_eq!(&bytes[1..=label.len()], label);
    assert_eq!(bytes[label.len() + 1], 1);
    assert_eq!(&bytes[label.len() + 2..label.len() + 18], &ACCOUNT);
    assert_eq!(&bytes[label.len() + 18..], &DEVICE);
    assert_ne!(
        aad.as_bytes(),
        Aad::biometric_unlock(1, &ACCOUNT, &DEVICE).as_bytes()
    );
}
