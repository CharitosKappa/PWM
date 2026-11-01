//! Secret Key generation and text format (D14).
//!
//! Expected strings were computed independently (BigInt base32 + mod 37), not
//! by this implementation.

use pwm_crypto::{CryptoError, Secret, SecretKey};

fn key(bytes: [u8; 16]) -> SecretKey {
    SecretKey::from_bytes(Secret::new(bytes))
}

fn hex16(s: &str) -> [u8; 16] {
    hex::decode(s).unwrap().try_into().unwrap()
}

const VECTORS: [(&str, &str); 4] = [
    (
        "00000000000000000000000000000000",
        "A1-000000-000000-000000-000000-000",
    ),
    (
        "ffffffffffffffffffffffffffffffff",
        "A1-7ZZZZZ-ZZZZZZ-ZZZZZZ-ZZZZZZ-ZZ*",
    ),
    (
        "000102030405060708090a0b0c0d0e0f",
        "A1-000410-61050R-3GG28A-1C60T3-GF6",
    ),
    (
        "0123456789abcdeffedcba9876543210",
        "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA",
    ),
];

#[test]
fn known_encodings() {
    for (hex, text) in VECTORS {
        assert_eq!(key(hex16(hex)).to_text().expose_secret(), text);
        let parsed = SecretKey::parse(text).unwrap();
        assert_eq!(parsed.to_text().expose_secret(), text);
    }
}

#[test]
fn generated_keys_round_trip_and_differ() {
    let a = SecretKey::generate().unwrap();
    let b = SecretKey::generate().unwrap();
    let text = a.to_text();
    assert_ne!(text.expose_secret(), b.to_text().expose_secret());
    let back = SecretKey::parse(text.expose_secret()).unwrap();
    assert_eq!(back.to_text().expose_secret(), text.expose_secret());
}

#[test]
fn text_shape() {
    let text = SecretKey::generate().unwrap().to_text();
    let groups: Vec<&str> = text.expose_secret().split('-').collect();
    let lens: Vec<usize> = groups.iter().map(|g| g.len()).collect();
    assert_eq!(groups[0], "A1");
    assert_eq!(lens, [2, 6, 6, 6, 6, 3]);
}

#[test]
fn parsing_is_forgiving_about_case_separators_and_lookalikes() {
    let canonical = "A1-000410-61050R-3GG28A-1C60T3-GF6";
    for input in [
        "a1-000410-61050r-3gg28a-1c60t3-gf6",
        "A1000410 61050R 3GG28A 1C60T3 GF6",
        " A1-OOO41O-61O5OR-3GG28A-1C6OT3-GF6 ", // O for 0
        "A1-000410-6I050R-3GG28A-lC60T3-GF6",   // I and l for 1
        "000410-61050R-3GG28A-1C60T3-GF6",      // prefix omitted
    ] {
        let parsed = SecretKey::parse(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
        assert_eq!(parsed.to_text().expose_secret(), canonical);
    }
}

#[test]
fn every_single_character_typo_is_caught() {
    let text = "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA";
    let alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ*~$=U";
    for (i, original) in text.char_indices().skip(3) {
        if original == '-' {
            continue;
        }
        for c in alphabet.chars().filter(|&c| c != original) {
            let mut typo = text.to_string();
            typo.replace_range(i..i + 1, &c.to_string());
            assert!(SecretKey::parse(&typo).is_err(), "{typo} accepted");
        }
    }
}

#[test]
fn adjacent_transpositions_are_caught() {
    let compact: Vec<char> = "014D2PF2DBSQQZXQ5TK1V58CGGA".chars().collect();
    for i in 0..compact.len() - 1 {
        if compact[i] == compact[i + 1] {
            continue;
        }
        let mut t = compact.clone();
        t.swap(i, i + 1);
        let s: String = t.into_iter().collect();
        assert!(SecretKey::parse(&format!("A1{s}")).is_err(), "swap at {i}");
    }
}

#[test]
fn malformed_input_is_rejected() {
    let cases = [
        (
            "A2-000000-000000-000000-000000-000",
            CryptoError::SecretKeyFormat,
        ), // unknown version
        (
            "A1-000000-000000-000000-000000-00",
            CryptoError::SecretKeyFormat,
        ), // too short
        (
            "A1-000000-000000-000000-000000-0000",
            CryptoError::SecretKeyFormat,
        ), // too long
        (
            "A1-U00000-000000-000000-000000-000",
            CryptoError::SecretKeyFormat,
        ), // U only valid as check
        (
            "A1-800000-000000-000000-000000-000",
            CryptoError::SecretKeyFormat,
        ), // > 128 bits
        (
            "A1-00000!-000000-000000-000000-000",
            CryptoError::SecretKeyFormat,
        ),
        ("", CryptoError::SecretKeyFormat),
        (
            "A1-000000-000000-000000-000000-001",
            CryptoError::SecretKeyChecksum,
        ),
    ];
    for (input, err) in cases {
        assert_eq!(SecretKey::parse(input).unwrap_err(), err, "{input:?}");
    }
}

#[test]
fn debug_is_redacted() {
    let k = key(hex16("0123456789abcdeffedcba9876543210"));
    let out = format!("{k:?}");
    assert!(out.contains("REDACTED"), "{out}");
    assert!(!out.contains("014D2P"), "{out}");
}
