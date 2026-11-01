//! Hashes for the health report (decision D22).

use pwm_crypto::{ReuseHasher, hibp_sha1};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

#[test]
fn hibp_sha1_matches_known_digests() {
    // Well-known SHA-1 digests (also the values HIBP lists).
    assert_eq!(
        hex(hibp_sha1(b"password").expose_secret()),
        "5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8"
    );
    assert_eq!(
        hex(hibp_sha1(b"").expose_secret()),
        "DA39A3EE5E6B4B0D3255BFEF95601890AFD80709"
    );
    assert_eq!(
        hex(hibp_sha1(b"abc").expose_secret()),
        "A9993E364706816ABA3E25717850C26C9CD0D89D"
    );
}

#[test]
fn reuse_hashes_match_only_within_one_hasher() {
    let a = ReuseHasher::new().unwrap();
    assert_eq!(a.hash(b"hunter2"), a.hash(b"hunter2"));
    assert_ne!(a.hash(b"hunter2"), a.hash(b"Hunter2"));

    // A fresh random key per run: the same password hashes differently.
    let b = ReuseHasher::new().unwrap();
    assert_ne!(a.hash(b"hunter2"), b.hash(b"hunter2"));
}

#[test]
fn reuse_hasher_debug_hides_the_key() {
    let debug = format!("{:?}", ReuseHasher::new().unwrap());
    assert!(debug.contains("REDACTED"), "{debug}");
}
