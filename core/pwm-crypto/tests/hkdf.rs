//! HKDF-SHA256 with 32-byte output (D13).

use pwm_crypto::hkdf_sha256;

fn unhex(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap()
}

/// RFC 5869 A.1 (test case 1); our output is the first 32 bytes of the OKM.
#[test]
fn rfc5869_case_1() {
    let okm = hkdf_sha256(
        &[0x0b; 22],
        &unhex("000102030405060708090a0b0c"),
        &unhex("f0f1f2f3f4f5f6f7f8f9"),
    );
    assert_eq!(
        okm.expose_secret().as_slice(),
        unhex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf")
    );
}

/// RFC 5869 A.3 (test case 3: empty salt and info).
#[test]
fn rfc5869_case_3() {
    let okm = hkdf_sha256(&[0x0b; 22], &[], &[]);
    assert_eq!(
        okm.expose_secret().as_slice(),
        unhex("8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d")
    );
}

#[test]
fn info_separates_outputs() {
    let ikm = [7u8; 32];
    let salt = [1u8; 16];
    let auk = hkdf_sha256(&ikm, &salt, b"pwm/auk/v1");
    let auth = hkdf_sha256(&ikm, &salt, b"pwm/auth/v1");
    assert_ne!(auk.expose_secret(), auth.expose_secret());
}
