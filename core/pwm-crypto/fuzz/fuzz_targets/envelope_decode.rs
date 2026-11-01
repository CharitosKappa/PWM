//! Envelope decoding: never panics, re-encodes losslessly, and decrypting
//! arbitrary input under a fixed key fails cleanly.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::{Aad, Envelope, Secret, SymmetricKey, open};

fuzz_target!(|data: &[u8]| {
    if let Ok(env) = Envelope::from_bytes(data) {
        assert_eq!(env.to_bytes(), data);
        let key = SymmetricKey::from_secret(Secret::new([7u8; 32]));
        let aad = Aad::item(1, &[1; 16], &[2; 16], 3);
        assert!(open(&key, &env, &aad).is_err());
    }
});
