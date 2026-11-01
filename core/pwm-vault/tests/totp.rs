//! TOTP codes from stored `otpauth://` URIs, bare secrets and Steam (D23).

use pwm_crypto::OtpAlgorithm;
use pwm_vault::{Item, ItemKind, Totp, TotpKind, VaultError, normalize_totp};
use serde_json::Value;

#[test]
fn totp_vectors() {
    let doc: Value = serde_json::from_str(include_str!("../../test-vectors/totp.json")).unwrap();
    let mut checked = 0;
    for v in doc["vectors"].as_array().unwrap() {
        if v["kind"] == "hotp" {
            continue;
        }
        let totp = Totp::parse(v["uri"].as_str().unwrap()).unwrap();
        let expected_kind = if v["kind"] == "steam" {
            TotpKind::Steam
        } else {
            TotpKind::Digits(v["code"].as_str().unwrap().len() as u8)
        };
        assert_eq!(totp.kind(), expected_kind, "{}", v["uri"]);
        let code = totp.code(v["time"].as_u64().unwrap());
        assert_eq!(
            code.code.expose_secret(),
            v["code"].as_str().unwrap(),
            "{}",
            v["uri"]
        );
        checked += 1;
    }
    assert_eq!(checked, 26);
}

const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

fn invalid(input: &str) -> bool {
    matches!(Totp::parse(input), Err(VaultError::Invalid(_)))
}

#[test]
fn defaults_follow_the_key_uri_format() {
    let totp = Totp::parse(&format!("otpauth://totp/Example:alice?secret={SECRET}")).unwrap();
    assert_eq!(totp.algorithm(), OtpAlgorithm::Sha1);
    assert_eq!(totp.kind(), TotpKind::Digits(6));
    assert_eq!(totp.period(), 30);
}

#[test]
fn parameters_are_read_case_insensitively() {
    let totp = Totp::parse(&format!(
        "OTPAUTH://TOTP/Issuer%20Name:bob%40example.com?Secret={}&ALGORITHM=sha512&Digits=8&period=60&issuer=Issuer%20Name&image=https%3A%2F%2Fx",
        SECRET.to_lowercase()
    ))
    .unwrap();
    assert_eq!(totp.algorithm(), OtpAlgorithm::Sha512);
    assert_eq!(totp.kind(), TotpKind::Digits(8));
    assert_eq!(totp.period(), 60);
    // Percent-encoded padding in the secret is accepted.
    assert!(Totp::parse(&format!("otpauth://totp/x?secret={SECRET}%3D%3D")).is_ok());
}

#[test]
fn seconds_left_counts_down_each_period() {
    let totp = Totp::parse(SECRET).unwrap();
    assert_eq!(totp.code(0).seconds_left, 30);
    assert_eq!(totp.code(29).seconds_left, 1);
    assert_eq!(totp.code(30).seconds_left, 30);
    assert_eq!(
        totp.code(30).code.expose_secret(),
        totp.code(59).code.expose_secret()
    );
    assert_ne!(
        totp.code(59).code.expose_secret(),
        totp.code(60).code.expose_secret()
    );
}

#[test]
fn bad_input_is_rejected() {
    assert!(invalid(""));
    assert!(invalid("   "));
    assert!(invalid("not base32!"));
    assert!(invalid("GEZDGNB1")); // '1' is not in the RFC 4648 alphabet
    assert!(invalid("otpauth://totp/x"));
    assert!(invalid("otpauth://totp/x?issuer=a"));
    assert!(invalid("otpauth://totp/x?secret="));
    for bad in [
        "digits=5",
        "digits=9",
        "digits=x",
        "period=0",
        "period=3601",
        "algorithm=MD5",
    ] {
        assert!(
            invalid(&format!("otpauth://totp/x?secret={SECRET}&{bad}")),
            "{bad}"
        );
    }
    assert!(invalid("steam://"));
}

#[test]
fn counter_based_and_unknown_schemes_are_unsupported() {
    for input in [
        format!("otpauth://hotp/x?secret={SECRET}&counter=3"),
        format!("otpauth://yaotp/x?secret={SECRET}"),
    ] {
        assert!(
            matches!(Totp::parse(&input), Err(VaultError::Unsupported(_))),
            "{input}"
        );
    }
}

#[test]
fn bare_secrets_are_saved_as_uris() {
    let stored = normalize_totp("gezd gnbv gy3t qojq gezd gnbv gy3t qojq", "Bank & Co").unwrap();
    assert_eq!(
        stored.expose_secret(),
        &format!("otpauth://totp/Bank%20%26%20Co?secret={SECRET}")
    );
    let from_uri = Totp::parse(stored.expose_secret()).unwrap();
    let from_bare = Totp::parse(SECRET).unwrap();
    assert_eq!(
        from_uri.code(1_234_567_890).code.expose_secret(),
        from_bare.code(1_234_567_890).code.expose_secret()
    );
}

#[test]
fn uris_are_saved_unchanged() {
    for uri in [
        format!(" otpauth://totp/A:b?secret={SECRET}&image=https%3A%2F%2Fx&future=1 "),
        format!("otpauth://hotp/A?secret={SECRET}&counter=7"),
        format!("steam://{SECRET}"),
    ] {
        assert_eq!(
            normalize_totp(&uri, "title").unwrap().expose_secret(),
            uri.trim()
        );
    }
    assert!(matches!(
        normalize_totp("otpauth://totp/x?secret=!!", "t"),
        Err(VaultError::Invalid(_))
    ));
    assert!(matches!(
        normalize_totp("hello, world!", "t"),
        Err(VaultError::Invalid(_))
    ));
}

#[test]
fn the_fixture_login_has_a_working_totp() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/items_v1.json")).unwrap();
    let item = Item::from_json(&fixtures[0].to_string()).unwrap();
    let ItemKind::Login(login) = &item.kind else {
        panic!("fixture 0 is a login");
    };
    let totp = Totp::parse(login.totp.as_deref().unwrap()).unwrap();
    assert_eq!(totp.code(0).code.expose_secret().len(), 6);
}

#[test]
fn debug_output_hides_the_secret_and_code() {
    let totp = Totp::parse(SECRET).unwrap();
    let code = totp.code(59);
    let debug = format!("{totp:?} {code:?}");
    assert!(
        !debug.contains("GEZDG") && !debug.contains("12345"),
        "{debug}"
    );
    assert!(
        !debug.contains(code.code.expose_secret().as_str()),
        "{debug}"
    );
}
