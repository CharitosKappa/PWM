//! The autofill rules file (D33): never panics on any bytes. Each input is
//! also signed with a test key first, so the JSON parser and the site
//! checks behind the signature get fuzzed too, not just the header.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_autofill::verify_rules;
use pwm_crypto::{Secret, SignatureContext, public_key_from_seed, sign_with_seed};

const SEED: [u8; 32] = [3; 32];

fuzz_target!(|data: &[u8]| {
    let public = public_key_from_seed(&Secret::new(SEED));
    let keys = [(1u8, public)];
    // As received.
    let _ = verify_rules(data, &keys, 0, 0);
    // As a validly signed file with this body.
    let message = [&[1u8][..], data].concat();
    let signature = sign_with_seed(
        &Secret::new(SEED),
        SignatureContext::AutofillRules,
        &message,
    );
    let signed = [&[1u8, 1][..], &signature, data].concat();
    if let Ok(rules) = verify_rules(&signed, &keys, 0, 0) {
        let _ = rules.version();
    }
});
