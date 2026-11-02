//! The signed rules file (D33 §7): signature, key pinning, rollback,
//! expiry, limits, and site lookup.

use pwm_autofill::{Class, Decision, RefuseReason, Rules, RulesError, rank, verify_rules};
use pwm_crypto::{Secret, SignatureContext, public_key_from_seed, sign_with_seed};
use pwm_vault::{LoginUrl, UrlMatch};

const SEED: [u8; 32] = [9; 32];
const NEXT_SEED: [u8; 32] = [10; 32];
const DAY: i64 = 24 * 60 * 60 * 1000;
const NOW: i64 = 1_767_225_600_000;

fn keys() -> Vec<(u8, [u8; 32])> {
    vec![
        (1, public_key_from_seed(&Secret::new(SEED))),
        (2, public_key_from_seed(&Secret::new(NEXT_SEED))),
    ]
}

/// `v || key_id || signature || json`, signed over `key_id || json`.
fn signed(json: &str, key_id: u8, seed: [u8; 32]) -> Vec<u8> {
    let message = [&[key_id][..], json.as_bytes()].concat();
    let signature = sign_with_seed(
        &Secret::new(seed),
        SignatureContext::AutofillRules,
        &message,
    );
    [&[1, key_id][..], &signature, json.as_bytes()].concat()
}

fn rules_json(version: u64, issued: i64, expires: i64, sites: &str) -> String {
    format!(
        r#"{{"v":1,"version":{version},"issued_at":{issued},"expires_at":{expires},"sites":[{sites}]}}"#
    )
}

const SITE: &str = r#"{"domain":"example.com","paths":["/login"],"fields":[{"attr":"id","equals":"acct","class":"username"}]},{"domain":"blocked.example","never_fill":true}"#;

fn good() -> String {
    rules_json(7, NOW - DAY, NOW + 30 * DAY, SITE)
}

#[test]
fn a_signed_file_is_accepted_and_looked_up() {
    let rules = verify_rules(&signed(&good(), 1, SEED), &keys(), NOW, 0).unwrap();
    assert_eq!(rules.version(), 7);
    let site = rules
        .site_for(&url::Url::parse("https://www.example.com/login/step").unwrap())
        .unwrap();
    assert_eq!(site.fields[0].class, Class::Username);
    assert!(
        rules
            .site_for(&url::Url::parse("https://www.example.com/other").unwrap())
            .is_none()
    );
    assert!(
        rules
            .site_for(&url::Url::parse("https://other.com/login").unwrap())
            .is_none()
    );
    // The next key works too, so the key can be rotated.
    assert!(verify_rules(&signed(&good(), 2, NEXT_SEED), &keys(), NOW, 0).is_ok());
}

#[test]
fn tampering_and_unknown_keys_are_refused() {
    let mut bytes = signed(&good(), 1, SEED);
    let last = bytes.len() - 3;
    bytes[last] ^= 1;
    assert_eq!(
        verify_rules(&bytes, &keys(), NOW, 0).unwrap_err(),
        RulesError::BadSignature
    );
    // Signed by key 1 but claiming to be key 2.
    let mut swapped = signed(&good(), 1, SEED);
    swapped[1] = 2;
    assert_eq!(
        verify_rules(&swapped, &keys(), NOW, 0).unwrap_err(),
        RulesError::BadSignature
    );
    assert_eq!(
        verify_rules(&signed(&good(), 3, SEED), &keys(), NOW, 0).unwrap_err(),
        RulesError::UnknownKey
    );
    // No production keys yet: nothing is accepted.
    assert_eq!(
        verify_rules(&signed(&good(), 1, SEED), &[], NOW, 0).unwrap_err(),
        RulesError::UnknownKey
    );
    assert_eq!(
        verify_rules(b"", &keys(), NOW, 0).unwrap_err(),
        RulesError::Malformed
    );
    let mut v2 = signed(&good(), 1, SEED);
    v2[0] = 2;
    assert_eq!(
        verify_rules(&v2, &keys(), NOW, 0).unwrap_err(),
        RulesError::UnsupportedVersion
    );
}

#[test]
fn older_and_expired_files_are_refused() {
    let file = signed(&good(), 1, SEED);
    assert!(
        verify_rules(&file, &keys(), NOW, 7).is_ok(),
        "the same version again is fine"
    );
    assert_eq!(
        verify_rules(&file, &keys(), NOW, 8).unwrap_err(),
        RulesError::Rollback
    );
    assert_eq!(
        verify_rules(&file, &keys(), NOW + 31 * DAY, 0).unwrap_err(),
        RulesError::Expired
    );
    let too_long = rules_json(7, NOW, NOW + 91 * DAY, SITE);
    assert_eq!(
        verify_rules(&signed(&too_long, 1, SEED), &keys(), NOW, 0).unwrap_err(),
        RulesError::Malformed
    );
    let backwards = rules_json(7, NOW, NOW - 1, SITE);
    assert_eq!(
        verify_rules(&signed(&backwards, 1, SEED), &keys(), NOW, 0).unwrap_err(),
        RulesError::Malformed
    );
}

#[test]
fn malformed_and_oversized_files_are_refused() {
    for sites in [
        r#"{"domain":"example.com","fields":[{"attr":"id","equals":"x","class":"nonsense"}]}"#,
        r#"{"domain":"example.com","fields":[{"attr":"css","equals":"x","class":"username"}]}"#,
        r#"{"domain":"example.com","fields":[{"attr":"id","class":"username"}]}"#,
        r#"{"domain":"co.uk"}"#,
        r#"{"domain":"www.example.com"}"#,
    ] {
        let json = rules_json(1, NOW, NOW + DAY, sites);
        assert_eq!(
            verify_rules(&signed(&json, 1, SEED), &keys(), NOW, 0).unwrap_err(),
            RulesError::Malformed,
            "{sites}"
        );
    }
    let wrong_v = good().replacen(r#""v":1"#, r#""v":2"#, 1);
    assert_eq!(
        verify_rules(&signed(&wrong_v, 1, SEED), &keys(), NOW, 0).unwrap_err(),
        RulesError::UnsupportedVersion
    );
    let huge = rules_json(
        1,
        NOW,
        NOW + DAY,
        &format!(
            r#"{{"domain":"example.com","paths":["{}"]}}"#,
            "a".repeat(1 << 20)
        ),
    );
    assert_eq!(
        verify_rules(&signed(&huge, 1, SEED), &keys(), NOW, 0).unwrap_err(),
        RulesError::TooLarge
    );
    let many: Vec<String> = (0..50_001)
        .map(|n| format!(r#"{{"domain":"s{n}.com"}}"#))
        .collect();
    let many = rules_json(1, NOW, NOW + DAY, &many.join(","));
    assert_eq!(
        verify_rules(&signed(&many, 1, SEED), &keys(), NOW, 0).unwrap_err(),
        RulesError::TooLarge
    );
}

#[test]
fn a_site_marked_never_fill_is_refused() {
    let rules = verify_rules(&signed(&good(), 1, SEED), &keys(), NOW, 0).unwrap();
    let mut url = LoginUrl::default();
    url.url = "blocked.example".to_owned();
    url.r#match = UrlMatch::Domain;
    let urls = vec![url];
    let items = vec![("a".to_owned(), urls.as_slice(), 0)];
    let got = rank("https://blocked.example/", None, &items, &rules).unwrap();
    assert_eq!(got[0].decision, Decision::Refuse(RefuseReason::SiteBlocked));
    let got = rank("https://blocked.example/", None, &items, &Rules::empty()).unwrap();
    assert_eq!(got[0].decision, Decision::Fill);
}

/// The frozen vector: a signer change or a format change shows up here.
const VECTOR: &str = include_str!("../../test-vectors/autofill_rules.json");

#[test]
fn vectors_autofill_rules() {
    let vector: serde_json::Value = serde_json::from_str(VECTOR).unwrap();
    let json = vector["json"].as_str().unwrap();
    let expected = hex(vector["signed_hex"].as_str().unwrap());
    let seed: [u8; 32] = hex(vector["seed_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    assert_eq!(signed(json, 1, seed), expected);
    let public: [u8; 32] = hex(vector["public_key_hex"].as_str().unwrap())
        .try_into()
        .unwrap();
    assert_eq!(public, public_key_from_seed(&Secret::new(seed)));
    let rules = verify_rules(
        &expected,
        &[(1, public)],
        vector["now_ms"].as_i64().unwrap(),
        0,
    )
    .unwrap();
    assert_eq!(rules.version(), vector["version"].as_u64().unwrap());
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

/// Prints a new vector; run with `--ignored --nocapture` only when the
/// format version changes.
#[test]
#[ignore = "writes a new vector"]
fn print_vector() {
    let json = good();
    let to_hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let out = serde_json::json!({
        "description": "Autofill rules file v1 (D33): v || key_id || Ed25519(label pwm/autofill-rules, key_id || json) || json",
        "seed_hex": to_hex(&SEED),
        "public_key_hex": to_hex(&public_key_from_seed(&Secret::new(SEED))),
        "json": json,
        "now_ms": NOW,
        "version": 7,
        "signed_hex": to_hex(&signed(&json, 1, SEED)),
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
