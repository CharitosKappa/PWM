//! Matching a login to an Android app (D58 §4): the package and the
//! signing certificate's SHA-256, never a guess from the package name.

use pwm_autofill::{AppLinkError, AppMatch, app_link, match_app};

const BANK: &str = "com.example.bank";
const CERT: &str = "F0:FD:6C:5B:41:0F:25:CB:25:C3:B5:33:46:C8:97:2F:AE:30:F8:EE:74:11:DF:91:04:80:AD:6B:2D:60:DB:83";
const CERT_HEX: &str = "f0fd6c5b410f25cb25c3b53346c8972fae30f8ee7411df910480ad6b2d60db83";
const OTHER: &str = "a78b62a5165b4494b2fead9e76a280d22d937fee6251aece599446b2ea319b04";

fn certs(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_link_names_the_package_and_the_certificate() {
    assert_eq!(
        app_link(BANK, CERT).unwrap(),
        format!("android-app://{BANK}?sha256={CERT_HEX}")
    );
    // Colons and case don't matter in the digest.
    assert_eq!(
        app_link(BANK, &CERT_HEX.to_uppercase()).unwrap(),
        app_link(BANK, CERT).unwrap()
    );
}

#[test]
fn bad_packages_and_digests_are_refused() {
    for package in [
        "",
        "bank",
        "com..bank",
        "com.1bank",
        "com.bank!",
        ".com.bank",
        "com.bank.",
    ] {
        assert_eq!(
            app_link(package, CERT),
            Err(AppLinkError::Package),
            "{package}"
        );
    }
    for digest in [
        "",
        "f0fd",
        "zz".repeat(32).as_str(),
        &format!("{CERT_HEX}00"),
    ] {
        assert_eq!(
            app_link(BANK, digest),
            Err(AppLinkError::Digest),
            "{digest}"
        );
    }
}

#[test]
fn the_same_package_and_certificate_match() {
    let link = app_link(BANK, CERT).unwrap();
    assert_eq!(
        match_app(BANK, &certs(&[CERT_HEX]), &link),
        Some(AppMatch::Linked)
    );
    // Any of the app's certificates (a rotated signing key keeps the old one).
    assert_eq!(
        match_app(BANK, &certs(&[OTHER, CERT]), &link),
        Some(AppMatch::Linked)
    );
}

#[test]
fn another_package_or_certificate_doesnt() {
    let link = app_link(BANK, CERT).unwrap();
    assert_eq!(
        match_app("com.example.bank2", &certs(&[CERT_HEX]), &link),
        None
    );
    assert_eq!(match_app("com.example", &certs(&[CERT_HEX]), &link), None);
    assert_eq!(
        match_app(BANK, &certs(&[OTHER]), &link),
        None,
        "same name, another signer"
    );
    assert_eq!(match_app(BANK, &[], &link), None);
}

#[test]
fn links_without_a_digest_are_only_unverified() {
    // From other managers' exports: offered from search, never suggested.
    assert_eq!(
        match_app(BANK, &certs(&[CERT_HEX]), &format!("androidapp://{BANK}")),
        Some(AppMatch::Unverified)
    );
    assert_eq!(
        match_app(BANK, &certs(&[CERT_HEX]), &format!("android-app://{BANK}")),
        Some(AppMatch::Unverified)
    );
    assert_eq!(
        match_app(
            "com.other",
            &certs(&[CERT_HEX]),
            &format!("androidapp://{BANK}")
        ),
        None
    );
}

#[test]
fn web_urls_and_junk_never_match_an_app() {
    for item in [
        "https://bank.example.com",
        "com.example.bank",
        "android-app://",
        "android-app://com.example.bank?sha256=",
        "android-app://com.example.bank?sha256=nothex",
        "android-app://com.example.bank/path?sha256=f0fd",
        "ANDROID-APP://com.example.bank",
    ] {
        assert_eq!(match_app(BANK, &certs(&[CERT_HEX]), item), None, "{item}");
    }
}
