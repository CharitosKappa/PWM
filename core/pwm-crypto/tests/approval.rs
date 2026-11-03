//! Approving a new device from a signed-in one (decision D46): the
//! commitment, the matching code and the sealed Secret Key.

use pwm_crypto::{
    Aad, ApprovalKeys, CryptoError, Id, Secret, approval_commitment, check_approval_commitment,
};

const APPROVAL: Id = [0xAB; 16];
const SECRET_KEY: &[u8] = b"A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA";

#[test]
fn both_devices_get_the_same_code_and_key() {
    let new = ApprovalKeys::generate().unwrap();
    let existing = ApprovalKeys::generate().unwrap();
    let (n_pub, e_pub) = (new.public(), existing.public());
    let at_new = new.channel(&APPROVAL, &n_pub, &e_pub).unwrap();
    let at_existing = existing.channel(&APPROVAL, &n_pub, &e_pub).unwrap();
    assert_eq!(at_new.code(), at_existing.code());
    assert!(at_new.code() < 1_000_000);

    let envelope = at_existing.seal(SECRET_KEY).unwrap();
    assert_eq!(at_new.open(&envelope).unwrap().expose_secret(), SECRET_KEY);
    // A fresh pair of keys gives another code and key.
    let other = ApprovalKeys::generate().unwrap();
    let elsewhere = other.channel(&APPROVAL, &other.public(), &e_pub).unwrap();
    assert!(elsewhere.open(&envelope).is_err());
}

#[test]
fn the_commitment_binds_the_new_device_key() {
    let new = ApprovalKeys::generate().unwrap();
    let commitment = approval_commitment(&new.public());
    assert_eq!(commitment, new.commitment());
    check_approval_commitment(&commitment, &new.public()).unwrap();
    let other = ApprovalKeys::generate().unwrap();
    assert!(matches!(
        check_approval_commitment(&commitment, &other.public()),
        Err(CryptoError::InvalidMessage)
    ));
}

#[test]
fn a_server_that_swaps_the_keys_shows_different_codes() {
    // The server gives E its own key `m1` instead of N's, and N `m2`
    // instead of E's: each side shares a channel with the server only.
    let new = ApprovalKeys::generate().unwrap();
    let existing = ApprovalKeys::generate().unwrap();
    let m1 = ApprovalKeys::generate().unwrap();
    let m2 = ApprovalKeys::generate().unwrap();
    let at_existing = existing
        .channel(&APPROVAL, &m1.public(), &existing.public())
        .unwrap();
    let at_new = new.channel(&APPROVAL, &new.public(), &m2.public()).unwrap();
    assert_ne!(at_existing.code(), at_new.code(), "the user sees two codes");
    let envelope = at_existing.seal(SECRET_KEY).unwrap();
    assert!(at_new.open(&envelope).is_err());
}

#[test]
fn the_envelope_is_bound_to_the_request_and_both_keys() {
    let new = ApprovalKeys::generate().unwrap();
    let existing = ApprovalKeys::generate().unwrap();
    let (n_pub, e_pub) = (new.public(), existing.public());
    let envelope = existing
        .channel(&APPROVAL, &n_pub, &e_pub)
        .unwrap()
        .seal(SECRET_KEY)
        .unwrap();
    let other_request = new.channel(&[0xAC; 16], &n_pub, &e_pub).unwrap();
    assert!(other_request.open(&envelope).is_err());
    let aad = Aad::approval(1, &APPROVAL, &n_pub, &e_pub);
    let label = b"pwm/approval";
    assert_eq!(aad.as_bytes()[0] as usize, label.len());
    assert_eq!(&aad.as_bytes()[1..=label.len()], label);
    assert_eq!(aad.as_bytes().len(), 1 + label.len() + 1 + 16 + 32 + 32);
}

#[test]
fn a_key_that_is_neither_side_or_of_small_order_is_refused() {
    let new = ApprovalKeys::generate().unwrap();
    let existing = ApprovalKeys::generate().unwrap();
    let stranger = ApprovalKeys::generate().unwrap();
    assert!(matches!(
        new.channel(&APPROVAL, &stranger.public(), &existing.public()),
        Err(CryptoError::InvalidMessage)
    ));
    assert!(matches!(
        new.channel(&APPROVAL, &new.public(), &[0u8; 32]),
        Err(CryptoError::InvalidPublicKey)
    ));
}

#[test]
fn codes_keep_their_leading_zeros_and_keys_never_print() {
    let keys = ApprovalKeys::from_seed(&Secret::new([0x11; 32]));
    let other = ApprovalKeys::from_seed(&Secret::new([0x22; 32]));
    let channel = keys
        .channel(&APPROVAL, &keys.public(), &other.public())
        .unwrap();
    let text = channel.code_text();
    assert_eq!(text.len(), 7);
    assert_eq!(text.as_bytes()[3], b' ');
    assert_eq!(text.replace(' ', ""), format!("{:06}", channel.code()));
    let printed = format!("{keys:?} {channel:?}");
    assert!(!printed.contains("11, 11"), "{printed}");
    assert!(!printed.contains(&channel.code().to_string()));
}
