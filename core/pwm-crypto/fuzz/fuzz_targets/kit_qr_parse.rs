//! The Emergency Kit's QR code text (D42): never panics, and any accepted
//! code, written out again, reads back the same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_kit::{Kit, parse_kit_qr};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(kit) = parse_kit_qr(text) else {
        return;
    };
    let again = Kit {
        email: &kit.email,
        secret_key: &kit.secret_key,
        server: kit.server.as_deref(),
        account_id: "00000000-0000-4000-8000-000000000000",
        created: "2026-11-02",
    }
    .qr_payload()
    .unwrap();
    let back = parse_kit_qr(again.expose_secret()).unwrap();
    assert_eq!(back.email, kit.email);
    assert_eq!(back.server, kit.server);
    assert_eq!(
        back.secret_key.to_text().expose_secret(),
        kit.secret_key.to_text().expose_secret()
    );
});
