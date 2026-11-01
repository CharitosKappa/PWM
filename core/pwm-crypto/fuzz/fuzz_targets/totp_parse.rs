//! TOTP parsing (D23): never panics on imported or pasted text; any accepted
//! value yields a code of the right shape, and a normalized value parses.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_vault::{Totp, TotpKind, VaultError, normalize_totp};

fuzz_target!(|data: &[u8]| {
    let Some((time, rest)) = data.split_first_chunk::<8>() else {
        return;
    };
    let Ok(input) = std::str::from_utf8(rest) else {
        return;
    };
    if let Ok(totp) = Totp::parse(input) {
        let code = totp.code(u64::from_le_bytes(*time));
        let len = match totp.kind() {
            TotpKind::Digits(d) => usize::from(d),
            TotpKind::Steam => 5,
        };
        assert_eq!(code.code.expose_secret().len(), len);
        assert!((1..=totp.period()).contains(&code.seconds_left));
    }
    if let Ok(stored) = normalize_totp(input, "title") {
        match Totp::parse(stored.expose_secret()) {
            Ok(_) | Err(VaultError::Unsupported(_)) => {}
            Err(e) => panic!("normalized value does not parse: {e}"),
        }
    }
});
