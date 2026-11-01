//! Password normalization: never panics, is idempotent, and accepted output
//! is non-empty with no control characters.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::normalize_password;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(normalized) = normalize_password(input) {
        let n = normalized.expose_secret();
        assert!(!n.is_empty() && !n.chars().any(char::is_control));
        let again = normalize_password(n).unwrap();
        assert_eq!(again.expose_secret(), n);
    }
});
