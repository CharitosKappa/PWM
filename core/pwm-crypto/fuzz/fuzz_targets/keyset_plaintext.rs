//! Keyset plaintext parsing: arbitrary bytes sealed as a keyset under a known
//! AUK, and as a biometric (D45) or browser (D48) unlock blob under a known
//! unlock key. Only a 97-byte version-1 plaintext may open, and it must
//! round-trip.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::{Aad, AccountKeyset, Secret, SymmetricKey, seal};

const ACCOUNT: [u8; 16] = [0xA1; 16];
const DEVICE: [u8; 16] = [0xD1; 16];

fuzz_target!(|data: &[u8]| {
    let valid = data.len() == 97 && data[0] == 1;
    let auk = SymmetricKey::from_secret(Secret::new([9u8; 32]));
    let sealed = seal(&auk, data, &Aad::keyset(1, &ACCOUNT)).unwrap();
    match AccountKeyset::open(&sealed, &auk, &ACCOUNT) {
        Ok(keyset) => {
            assert!(valid);
            let resealed = keyset.seal(&auk, &ACCOUNT).unwrap();
            let reopened = AccountKeyset::open(&resealed, &auk, &ACCOUNT).unwrap();
            assert_eq!(reopened.ed25519_public(), keyset.ed25519_public());
            assert_eq!(reopened.x25519_public(), keyset.x25519_public());
        }
        Err(_) => assert!(!valid),
    }

    let unlock_key = SymmetricKey::from_secret(Secret::new([7u8; 32]));
    let blob = seal(
        &unlock_key,
        data,
        &Aad::biometric_unlock(1, &ACCOUNT, &DEVICE),
    )
    .unwrap();
    match AccountKeyset::import_biometric_unlock(&blob, &Secret::new([7u8; 32]), &ACCOUNT, &DEVICE)
    {
        Ok(keyset) => {
            assert!(valid);
            let (again, key) = keyset.export_biometric_unlock(&ACCOUNT, &DEVICE).unwrap();
            let reopened =
                AccountKeyset::import_biometric_unlock(&again, &key, &ACCOUNT, &DEVICE).unwrap();
            assert_eq!(reopened.x25519_public(), keyset.x25519_public());
        }
        Err(_) => assert!(!valid),
    }

    let browser = seal(
        &unlock_key,
        data,
        &Aad::browser_unlock(1, &ACCOUNT, &DEVICE),
    )
    .unwrap();
    match AccountKeyset::import_browser_unlock(&browser, &Secret::new([7u8; 32]), &ACCOUNT, &DEVICE)
    {
        Ok(keyset) => {
            assert!(valid);
            let (again, key) = keyset.export_browser_unlock(&ACCOUNT, &DEVICE).unwrap();
            let reopened =
                AccountKeyset::import_browser_unlock(&again, &key, &ACCOUNT, &DEVICE).unwrap();
            assert_eq!(reopened.x25519_public(), keyset.x25519_public());
        }
        Err(_) => assert!(!valid),
    }
    // The two blobs never open each other.
    assert!(
        AccountKeyset::import_biometric_unlock(
            &browser,
            &Secret::new([7u8; 32]),
            &ACCOUNT,
            &DEVICE
        )
        .is_err()
    );
});
