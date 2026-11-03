//! Checking a one-time-code setup before an item is saved (D23 §3, D43 §2):
//! the app stores what `normalize_totp` returns.

use pwm_client::{ClientError, normalize_totp};

#[test]
fn a_bare_secret_becomes_a_uri_named_after_the_item() {
    let stored = normalize_totp(" JBSW Y3DP EHPK 3PXP ", "Bank & Co").unwrap();
    assert_eq!(
        stored.expose_secret(),
        "otpauth://totp/Bank%20%26%20Co?secret=JBSWY3DPEHPK3PXP"
    );
}

#[test]
fn uris_are_kept_as_entered() {
    for uri in [
        "otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP&issuer=Example&image=x",
        "steam://JBSWY3DPEHPK3PXP",
        // Valid but not supported yet: kept, shown as an unsupported code.
        "otpauth://hotp/Example?secret=JBSWY3DPEHPK3PXP&counter=1",
    ] {
        assert_eq!(normalize_totp(uri, "t").unwrap().expose_secret(), uri);
    }
}

#[test]
fn anything_else_is_refused_without_the_input() {
    for input in ["", "hello, world!", "otpauth://totp/x?secret=!!"] {
        let err = normalize_totp(input, "t").unwrap_err();
        assert!(matches!(err, ClientError::InvalidTotp), "{input:?}");
        assert_eq!(err.code(), "invalid_totp");
        assert!(!format!("{err:?} {err}").contains("hello"));
    }
}
