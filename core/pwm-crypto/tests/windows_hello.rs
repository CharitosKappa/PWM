//! Windows Hello unlock (D68): the biometric unlock key sealed under a key
//! derived from a deterministic signature over a stored challenge.

use pwm_crypto::{
    Aad, CryptoError, Envelope, Secret, hkdf_sha256, open_windows_hello, seal_windows_hello,
    windows_hello_challenge,
};

const ACCOUNT: [u8; 16] = [0xA1; 16];
const DEVICE: [u8; 16] = [0xD1; 16];

/// A stand-in for an RSA-2048 PKCS#1 v1.5 signature (256 bytes).
fn signature(byte: u8) -> Vec<u8> {
    vec![byte; 256]
}

fn unlock_key() -> Secret<[u8; 32]> {
    Secret::new([0x77; 32])
}

#[test]
fn the_unlock_key_comes_back_with_the_same_signature() {
    let challenge = windows_hello_challenge().unwrap();
    let sealed =
        seal_windows_hello(&unlock_key(), &signature(1), &challenge, &ACCOUNT, &DEVICE).unwrap();
    let opened = open_windows_hello(&sealed, &signature(1), &challenge, &ACCOUNT, &DEVICE).unwrap();
    assert_eq!(opened.expose_secret(), unlock_key().expose_secret());
}

#[test]
fn another_signature_challenge_account_or_device_is_refused() {
    let challenge = windows_hello_challenge().unwrap();
    let sealed =
        seal_windows_hello(&unlock_key(), &signature(1), &challenge, &ACCOUNT, &DEVICE).unwrap();
    let other_challenge = windows_hello_challenge().unwrap();
    assert_ne!(challenge.expose_secret(), other_challenge.expose_secret());
    for (sig, ch, account, device) in [
        (signature(2), &challenge, ACCOUNT, DEVICE),
        (signature(1), &other_challenge, ACCOUNT, DEVICE),
        (signature(1), &challenge, [0xA2; 16], DEVICE),
        (signature(1), &challenge, ACCOUNT, [0xD2; 16]),
    ] {
        assert!(open_windows_hello(&sealed, &sig, ch, &account, &device).is_err());
    }
}

#[test]
fn a_short_signature_is_never_a_key() {
    let challenge = windows_hello_challenge().unwrap();
    for short in [vec![], vec![1; 63]] {
        assert_eq!(
            seal_windows_hello(&unlock_key(), &short, &challenge, &ACCOUNT, &DEVICE).unwrap_err(),
            CryptoError::InvalidMessage
        );
    }
}

#[test]
fn the_layout_is_hkdf_then_the_v1_envelope() {
    // The wrapping key is HKDF-SHA256(signature, challenge, label), and the
    // AAD binds the version, the account and the device.
    let challenge = Secret::new([0x33; 32]);
    let sealed =
        seal_windows_hello(&unlock_key(), &signature(9), &challenge, &ACCOUNT, &DEVICE).unwrap();
    let key = hkdf_sha256(&signature(9), &[0x33; 32], b"pwm/windows-hello/v1");
    let opened = pwm_crypto::open(
        &pwm_crypto::SymmetricKey::from_secret(key),
        &Envelope::from_bytes(&sealed.to_bytes()).unwrap(),
        &Aad::windows_hello(1, &ACCOUNT, &DEVICE),
    )
    .unwrap();
    assert_eq!(opened.expose_secret().as_slice(), &[0x77; 32]);
    let aad = Aad::windows_hello(1, &ACCOUNT, &DEVICE);
    assert_eq!(&aad.as_bytes()[..18], b"\x11pwm/windows-hello");
}
