//! Passkey requests (D37): the page's WebAuthn options never panic the
//! parser, and whatever parses either registers and signs or is refused,
//! for any origin. Any RP id a platform passes (D47) is either refused or
//! registers and signs.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_crypto::Secret;
use pwm_passkey::{
    Caller, CreateRequest, GetRequest, authenticate, authenticate_hashed, effective_rp_id,
    register, register_hashed,
};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (origin, json) = text
        .split_once('\n')
        .unwrap_or(("https://example.com", text));
    let _ = effective_rp_id(origin, Some(json));
    if let Ok(hashed) = register_hashed(origin, b"u", "a", &Secret::new([0x42; 32]), [7; 16]) {
        let rp_id = hashed.passkey.rp_id.clone();
        assert!(authenticate_hashed(&rp_id, &[0; 32], &hashed.passkey).is_ok());
    }
    let caller = Caller {
        origin,
        top_origin: None,
    };
    if let Ok(request) = CreateRequest::from_json(json)
        && let Ok(registration) = register(caller, &request, &Secret::new([0x42; 32]), [7; 16])
    {
        let get = format!(
            r#"{{"challenge":"AAAA","rpId":"{}"}}"#,
            registration.passkey.rp_id
        );
        let get = GetRequest::from_json(&get).unwrap();
        assert!(authenticate(caller, &get, &registration.passkey).is_ok());
    }
    if let Ok(request) = GetRequest::from_json(json) {
        let _ = request.allow.len();
    }
});
