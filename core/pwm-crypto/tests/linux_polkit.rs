//! Unlock on Linux (D71): the root key holder answers a challenge with
//! `HKDF-SHA256(ikm = S, salt = challenge, info =
//! "pwm/linux-polkit/response/v1")`, and D68's and D69's constructions take
//! that answer in place of a Windows Hello signature, with Linux's labels.

use pwm_crypto::{
    Aad, Secret, Signer, hkdf_sha256, linux_polkit_response, linux_polkit_secret,
    open_signed_browser, open_signed_unlock, seal_signed_browser, seal_signed_unlock,
    windows_hello_challenge,
};

const ACCOUNT: [u8; 16] = [0xA1; 16];
const DEVICE: [u8; 16] = [0xD1; 16];
const CALLER: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn key() -> Secret<[u8; 32]> {
    Secret::new([0x5A; 32])
}

#[test]
fn the_answer_is_fixed_for_a_secret_and_a_challenge() {
    let secret = linux_polkit_secret().unwrap();
    let challenge = [0x6D; 32];
    let a = linux_polkit_response(&secret, &challenge);
    let b = linux_polkit_response(&secret, &challenge);
    assert_eq!(a.expose_secret(), b.expose_secret());
    assert_eq!(
        a.expose_secret(),
        hkdf_sha256(
            secret.expose_secret(),
            &challenge,
            b"pwm/linux-polkit/response/v1"
        )
        .expose_secret()
    );
    // Another challenge or another secret answers differently.
    let other = linux_polkit_response(&secret, &[0x6E; 32]);
    assert_ne!(a.expose_secret(), other.expose_secret());
    let other_secret = linux_polkit_secret().unwrap();
    assert_ne!(
        a.expose_secret(),
        linux_polkit_response(&other_secret, &challenge).expose_secret()
    );
}

#[test]
fn the_desktop_unlock_key_comes_back_with_the_same_answer() {
    let secret = linux_polkit_secret().unwrap();
    let challenge = windows_hello_challenge().unwrap();
    let answer = linux_polkit_response(&secret, challenge.expose_secret());
    let sealed = seal_signed_unlock(
        Signer::LinuxPolkit,
        &key(),
        answer.expose_secret(),
        &challenge,
        &ACCOUNT,
        &DEVICE,
    )
    .unwrap();
    let opened = open_signed_unlock(
        Signer::LinuxPolkit,
        &sealed,
        answer.expose_secret(),
        &challenge,
        &ACCOUNT,
        &DEVICE,
    )
    .unwrap();
    assert_eq!(opened.expose_secret(), key().expose_secret());

    // Another secret's answer, account or device fails.
    let other = linux_polkit_response(&linux_polkit_secret().unwrap(), challenge.expose_secret());
    for (answer, account, device) in [
        (other.expose_secret(), ACCOUNT, DEVICE),
        (answer.expose_secret(), [0xA2; 16], DEVICE),
        (answer.expose_secret(), ACCOUNT, [0xD2; 16]),
    ] {
        assert!(
            open_signed_unlock(
                Signer::LinuxPolkit,
                &sealed,
                answer,
                &challenge,
                &account,
                &device
            )
            .is_err()
        );
    }
}

#[test]
fn the_browser_key_comes_back_for_the_same_caller_only() {
    let challenge = windows_hello_challenge().unwrap();
    let answer = [0x42; 32];
    let sealed = seal_signed_browser(
        Signer::LinuxPolkit,
        &key(),
        &answer,
        &challenge,
        CALLER,
        &ACCOUNT,
        &DEVICE,
    )
    .unwrap();
    let opened = open_signed_browser(
        Signer::LinuxPolkit,
        &sealed,
        &answer,
        &challenge,
        CALLER,
        &ACCOUNT,
        &DEVICE,
    )
    .unwrap();
    assert_eq!(opened.expose_secret(), key().expose_secret());
    assert!(
        open_signed_browser(
            Signer::LinuxPolkit,
            &sealed,
            &answer,
            &challenge,
            "pwm@pwm.invalid",
            &ACCOUNT,
            &DEVICE,
        )
        .is_err()
    );
    // The desktop's unlock can't open a browser key, nor the reverse.
    assert!(
        open_signed_unlock(
            Signer::LinuxPolkit,
            &sealed,
            &answer,
            &challenge,
            &ACCOUNT,
            &DEVICE
        )
        .is_err()
    );
}

#[test]
fn an_answer_is_exactly_32_bytes() {
    let challenge = windows_hello_challenge().unwrap();
    for len in [0, 31, 33, 256] {
        assert!(
            seal_signed_unlock(
                Signer::LinuxPolkit,
                &key(),
                &vec![1; len],
                &challenge,
                &ACCOUNT,
                &DEVICE
            )
            .is_err(),
            "{len}"
        );
    }
}

#[test]
fn linux_and_windows_keep_their_own_labels() {
    // Windows' labels are unchanged (D68, D69); Linux has its own.
    assert_eq!(
        Aad::signed_unlock(Signer::WindowsHello, 1, &ACCOUNT, &DEVICE),
        Aad::windows_hello(1, &ACCOUNT, &DEVICE)
    );
    assert_eq!(
        Aad::signed_browser(Signer::WindowsHello, 1, CALLER, &ACCOUNT, &DEVICE).unwrap(),
        Aad::browser_hello(1, CALLER, &ACCOUNT, &DEVICE).unwrap()
    );
    let linux = Aad::signed_unlock(Signer::LinuxPolkit, 1, &ACCOUNT, &DEVICE);
    assert!(linux.as_bytes()[1..].starts_with(b"pwm/linux-polkit"));
    let browser = Aad::signed_browser(Signer::LinuxPolkit, 1, CALLER, &ACCOUNT, &DEVICE).unwrap();
    assert!(browser.as_bytes()[1..].starts_with(b"pwm/browser-unlock/linux-polkit"));

    // The same 64 bytes as a Windows signature and as a Linux answer never
    // open each other's envelope: Linux takes only 32-byte answers, and the
    // labels differ.
    let challenge = windows_hello_challenge().unwrap();
    let windows = seal_signed_unlock(
        Signer::WindowsHello,
        &key(),
        &[7; 64],
        &challenge,
        &ACCOUNT,
        &DEVICE,
    )
    .unwrap();
    assert!(
        open_signed_unlock(
            Signer::LinuxPolkit,
            &windows,
            &[7; 32],
            &challenge,
            &ACCOUNT,
            &DEVICE
        )
        .is_err()
    );
}
