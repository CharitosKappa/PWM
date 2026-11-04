//! Bitwarden's password-protected export (D51 §4): never panic on a crafted
//! file, whatever its KDF settings and EncStrings. KDF settings outside the
//! bounds are refused before the KDF runs, so each input stays cheap.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::Secret;
use pwm_import::{Format, Options, parse};

fuzz_target!(|data: &[u8]| {
    let options = Options {
        password: Some(Secret::new("correct horse battery staple 42".to_owned())),
        ..Options::default()
    };
    let _ = parse(Format::Bitwarden, data, &options);
});
