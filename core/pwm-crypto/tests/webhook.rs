//! The billing webhooks' checks (D6 §3, §10).

use pwm_crypto::{secret_matches, webhook_signature_valid};

/// RFC 4231 test case 1.
#[test]
fn rfc4231_case_1() {
    assert!(webhook_signature_valid(
        &[0x0b; 20],
        b"Hi There",
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
    ));
}

/// RFC 4231 test case 2, with uppercase hex too.
#[test]
fn rfc4231_case_2() {
    let mac = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
    assert!(webhook_signature_valid(
        b"Jefe",
        b"what do ya want for nothing?",
        mac
    ));
    assert!(webhook_signature_valid(
        b"Jefe",
        b"what do ya want for nothing?",
        &mac.to_uppercase()
    ));
}

#[test]
fn a_wrong_signature_secret_or_payload_fails() {
    let mac = "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
    let payload = b"what do ya want for nothing?";
    assert!(!webhook_signature_valid(b"Jeff", payload, mac));
    assert!(!webhook_signature_valid(
        b"Jefe",
        b"what do ya want for nothing!",
        mac
    ));
    let mut flipped = mac.to_owned();
    flipped.replace_range(0..1, "4");
    assert!(!webhook_signature_valid(b"Jefe", payload, &flipped));
    // Short, long, empty and non-hex signatures.
    assert!(!webhook_signature_valid(b"Jefe", payload, &mac[..62]));
    assert!(!webhook_signature_valid(
        b"Jefe",
        payload,
        &format!("{mac}00")
    ));
    assert!(!webhook_signature_valid(b"Jefe", payload, ""));
    assert!(!webhook_signature_valid(
        b"Jefe",
        payload,
        &mac.replace('5', "g")
    ));
}

#[test]
fn secrets_match_only_when_equal() {
    assert!(secret_matches(b"a-long-push-secret", b"a-long-push-secret"));
    assert!(!secret_matches(
        b"a-long-push-secret",
        b"a-long-push-secreT"
    ));
    assert!(!secret_matches(b"a-long-push-secret", b"a-long-push-secre"));
    assert!(!secret_matches(b"", b"a-long-push-secret"));
    // An empty expected secret never matches, even an empty one.
    assert!(!secret_matches(b"", b""));
}
