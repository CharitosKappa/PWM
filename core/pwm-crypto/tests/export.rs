//! Password-protected `.pwmx` export files (decision D24).

use pwm_crypto::{CryptoError, EXPORT_MAGIC, ExportHeader, KdfParams, open_export, seal_export};

const PASSWORD: &str = "orbit-velvet-candle-harbor-ninth";

fn sealed(plaintext: &[u8]) -> Vec<u8> {
    seal_export(PASSWORD, KdfParams::DEFAULT, plaintext).unwrap()
}

#[test]
fn round_trip() {
    let file = sealed(b"{\"vaults\":[]}");
    assert_eq!(&file[..4], &EXPORT_MAGIC);
    assert_eq!(file[4], ExportHeader::VERSION);
    let plaintext = open_export(PASSWORD, &file).unwrap();
    assert_eq!(plaintext.expose_secret().as_slice(), b"{\"vaults\":[]}");
}

#[test]
fn header_records_params_and_a_fresh_salt() {
    let a = sealed(b"x");
    let b = sealed(b"x");
    let (header_a, _) = ExportHeader::parse(&a).unwrap();
    let (header_b, _) = ExportHeader::parse(&b).unwrap();
    assert_eq!(header_a.params, KdfParams::DEFAULT);
    assert_ne!(header_a.salt, header_b.salt);
    assert_eq!(a.len(), ExportHeader::LEN + 41 + 1);
}

#[test]
fn wrong_password_fails() {
    let file = sealed(b"secret");
    assert_eq!(
        open_export("orbit-velvet-candle-harbor-tenth", &file).unwrap_err(),
        CryptoError::DecryptionFailed
    );
}

#[test]
fn password_is_normalized() {
    // Composed and decomposed accents, and a no-break space, are the same password.
    let file = seal_export("caf\u{e9}\u{a0}export", KdfParams::MOBILE_FLOOR, b"ok").unwrap();
    let plaintext = open_export("cafe\u{301} export", &file).unwrap();
    assert_eq!(plaintext.expose_secret().as_slice(), b"ok");
    assert_eq!(
        seal_export("", KdfParams::MOBILE_FLOOR, b"ok").unwrap_err(),
        CryptoError::PasswordRejected
    );
}

#[test]
fn changed_header_fails() {
    let file = sealed(b"secret");
    // Salt byte.
    let mut tampered = file.clone();
    tampered[20] ^= 1;
    assert_eq!(
        open_export(PASSWORD, &tampered).unwrap_err(),
        CryptoError::DecryptionFailed
    );
    // Iterations 3 -> 4: still valid params, but bound by the AAD.
    let mut tampered = file.clone();
    tampered[12] = 4;
    assert_eq!(
        open_export(PASSWORD, &tampered).unwrap_err(),
        CryptoError::DecryptionFailed
    );
}

#[test]
fn other_files_are_rejected_before_key_derivation() {
    let file = sealed(b"secret");
    assert_eq!(
        ExportHeader::parse(b"PK\x03\x04").unwrap_err(),
        CryptoError::NotExportFile
    );
    assert_eq!(
        ExportHeader::parse(b"PWM").unwrap_err(),
        CryptoError::NotExportFile
    );
    assert_eq!(
        ExportHeader::parse(b"PWMX").unwrap_err(),
        CryptoError::Truncated
    );
    let mut newer = file.clone();
    newer[4] = 2;
    assert_eq!(
        ExportHeader::parse(&newer).unwrap_err(),
        CryptoError::UnsupportedVersion(2)
    );
    assert_eq!(
        ExportHeader::parse(&file[..ExportHeader::LEN]).unwrap_err(),
        CryptoError::Truncated
    );
    assert_eq!(
        ExportHeader::parse(&file[..ExportHeader::LEN + 40]).unwrap_err(),
        CryptoError::Truncated
    );
}

#[test]
fn params_outside_the_account_bounds_are_rejected() {
    let file = sealed(b"secret");
    // Memory 64 MiB -> 4 GiB - 1 KiB: would exhaust the device.
    let mut huge = file.clone();
    huge[5..9].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        ExportHeader::parse(&huge).unwrap_err(),
        CryptoError::KdfParamsRejected
    );
    // Memory 64 MiB -> 1 MiB: too weak.
    let mut weak = file.clone();
    weak[5..9].copy_from_slice(&1024u32.to_be_bytes());
    assert_eq!(
        open_export(PASSWORD, &weak).unwrap_err(),
        CryptoError::KdfParamsRejected
    );
    let weak_params = KdfParams {
        memory_kib: 1024,
        iterations: 1,
        parallelism: 1,
    };
    assert_eq!(
        seal_export(PASSWORD, weak_params, b"x").unwrap_err(),
        CryptoError::KdfParamsRejected
    );
}
