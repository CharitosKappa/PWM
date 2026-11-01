//! Keyset plaintext parsing: arbitrary bytes sealed as a keyset under a known
//! AUK. Only a 97-byte version-1 plaintext may open, and it must round-trip.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::{Aad, AccountKeyset, Secret, SymmetricKey, seal};

const ACCOUNT: [u8; 16] = [0xA1; 16];

fuzz_target!(|data: &[u8]| {
    let auk = SymmetricKey::from_secret(Secret::new([9u8; 32]));
    let sealed = seal(&auk, data, &Aad::keyset(1, &ACCOUNT)).unwrap();
    match AccountKeyset::open(&sealed, &auk, &ACCOUNT) {
        Ok(keyset) => {
            assert!(data.len() == 97 && data[0] == 1);
            let resealed = keyset.seal(&auk, &ACCOUNT).unwrap();
            let reopened = AccountKeyset::open(&resealed, &auk, &ACCOUNT).unwrap();
            assert_eq!(reopened.ed25519_public(), keyset.ed25519_public());
            assert_eq!(reopened.x25519_public(), keyset.x25519_public());
        }
        Err(_) => assert!(data.len() != 97 || data[0] != 1),
    }
});
