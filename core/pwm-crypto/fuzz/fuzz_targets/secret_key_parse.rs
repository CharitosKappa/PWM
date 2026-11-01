//! Secret Key parsing: never panics, and any accepted input prints a
//! canonical form that parses back to the same key.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::SecretKey;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(key) = SecretKey::parse(input) {
        let text = key.to_text();
        let again = SecretKey::parse(text.expose_secret()).unwrap();
        assert_eq!(again.to_text().expose_secret(), text.expose_secret());
    }
});
