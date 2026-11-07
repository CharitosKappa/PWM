//! Browser unlock on Windows (D69): the extension's key K sealed under a
//! key derived from a Windows Hello signature, as D68 does, with its own
//! label and bound to the calling extension.

use pwm_crypto::{
    Aad, CryptoError, Envelope, Secret, hkdf_sha256, open_browser_hello, open_windows_hello,
    seal_browser_hello, seal_windows_hello, windows_hello_challenge,
};

const ACCOUNT: [u8; 16] = [0xA1; 16];
const DEVICE: [u8; 16] = [0xD1; 16];
const CALLER: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn signature(byte: u8) -> Vec<u8> {
    vec![byte; 256]
}

fn key() -> Secret<[u8; 32]> {
    Secret::new([0x5A; 32])
}

#[test]
fn the_key_comes_back_with_the_same_signature_and_caller() {
    let challenge = windows_hello_challenge().unwrap();
    let sealed =
        seal_browser_hello(&key(), &signature(1), &challenge, CALLER, &ACCOUNT, &DEVICE).unwrap();
    let opened = open_browser_hello(
        &sealed,
        &signature(1),
        &challenge,
        CALLER,
        &ACCOUNT,
        &DEVICE,
    )
    .unwrap();
    assert_eq!(opened.expose_secret(), key().expose_secret());
}

#[test]
fn another_signature_challenge_caller_account_or_device_is_refused() {
    let challenge = windows_hello_challenge().unwrap();
    let other_challenge = windows_hello_challenge().unwrap();
    let sealed =
        seal_browser_hello(&key(), &signature(1), &challenge, CALLER, &ACCOUNT, &DEVICE).unwrap();
    let other_caller = "moz-extension-id@pwm";
    for (sig, ch, caller, account, device) in [
        (signature(2), &challenge, CALLER, ACCOUNT, DEVICE),
        (signature(1), &other_challenge, CALLER, ACCOUNT, DEVICE),
        (signature(1), &challenge, other_caller, ACCOUNT, DEVICE),
        (signature(1), &challenge, CALLER, [0xA2; 16], DEVICE),
        (signature(1), &challenge, CALLER, ACCOUNT, [0xD2; 16]),
    ] {
        assert!(open_browser_hello(&sealed, &sig, ch, caller, &account, &device).is_err());
    }
}

#[test]
fn a_desktop_unlock_key_and_a_browser_key_never_stand_in_for_each_other() {
    let challenge = windows_hello_challenge().unwrap();
    let browser =
        seal_browser_hello(&key(), &signature(1), &challenge, CALLER, &ACCOUNT, &DEVICE).unwrap();
    assert!(open_windows_hello(&browser, &signature(1), &challenge, &ACCOUNT, &DEVICE).is_err());
    let desktop = seal_windows_hello(&key(), &signature(1), &challenge, &ACCOUNT, &DEVICE).unwrap();
    assert!(
        open_browser_hello(
            &desktop,
            &signature(1),
            &challenge,
            CALLER,
            &ACCOUNT,
            &DEVICE
        )
        .is_err()
    );
}

#[test]
fn a_short_signature_or_a_long_caller_is_refused() {
    let challenge = windows_hello_challenge().unwrap();
    assert_eq!(
        seal_browser_hello(&key(), &[1; 63], &challenge, CALLER, &ACCOUNT, &DEVICE).unwrap_err(),
        CryptoError::InvalidMessage
    );
    let long = "x".repeat(1025);
    assert_eq!(
        seal_browser_hello(&key(), &signature(1), &challenge, &long, &ACCOUNT, &DEVICE)
            .unwrap_err(),
        CryptoError::InvalidMessage
    );
}

#[test]
fn the_layout_is_hkdf_then_the_v1_envelope_with_the_caller_in_the_aad() {
    let challenge = Secret::new([0x33; 32]);
    let sealed =
        seal_browser_hello(&key(), &signature(9), &challenge, CALLER, &ACCOUNT, &DEVICE).unwrap();
    let wrapping = hkdf_sha256(
        &signature(9),
        &[0x33; 32],
        b"pwm/browser-unlock/windows-hello/v1",
    );
    let aad = Aad::browser_hello(1, CALLER, &ACCOUNT, &DEVICE).unwrap();
    let opened = pwm_crypto::open(
        &pwm_crypto::SymmetricKey::from_secret(wrapping),
        &Envelope::from_bytes(&sealed.to_bytes()).unwrap(),
        &aad,
    )
    .unwrap();
    assert_eq!(opened.expose_secret().as_slice(), &[0x5A; 32]);
    let label = b"pwm/browser-unlock/windows-hello";
    let bytes = aad.as_bytes();
    assert_eq!(usize::from(bytes[0]), label.len());
    assert_eq!(&bytes[1..=label.len()], label);
    let rest = &bytes[1 + label.len()..];
    assert_eq!(rest[0], 1);
    let len = u16::from_be_bytes([rest[1], rest[2]]);
    assert_eq!(usize::from(len), CALLER.len());
    assert_eq!(&rest[3..3 + CALLER.len()], CALLER.as_bytes());
    assert_eq!(&rest[3 + CALLER.len()..], [[0xA1; 16], [0xD1; 16]].concat());
}
