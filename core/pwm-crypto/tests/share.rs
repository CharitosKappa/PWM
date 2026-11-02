//! Ed25519 signatures and X25519 vault shares (decision D17).

use pwm_crypto::{
    Aad, AccountKeyset, CryptoError, Envelope, Id, SignatureContext, SymmetricKey, VaultShare,
    open, seal, verify_signature,
};

const VAULT: Id = [0xB1; 16];
const OTHER_VAULT: Id = [0xB2; 16];

fn keyset() -> AccountKeyset {
    AccountKeyset::generate().unwrap()
}

/// The signed message, built from the spec rather than the implementation.
fn share_sig_message(vault_id: &Id, recipient: &[u8; 32], share_bytes: &[u8]) -> Vec<u8> {
    let eph_pub = &share_bytes[1..33];
    let envelope = &share_bytes[97..];
    let mut msg = vec![1u8];
    msg.extend_from_slice(vault_id);
    msg.extend_from_slice(recipient);
    msg.extend_from_slice(eph_pub);
    msg.extend_from_slice(envelope);
    msg
}

// --- Signatures ---

#[test]
fn sign_and_verify() {
    let alice = keyset();
    let sig = alice.sign(SignatureContext::Share, b"message");
    assert_eq!(
        verify_signature(
            &alice.ed25519_public(),
            SignatureContext::Share,
            b"message",
            &sig
        ),
        Ok(())
    );
}

#[test]
fn verify_rejects_wrong_message_key_or_signature() {
    let alice = keyset();
    let sig = alice.sign(SignatureContext::Share, b"message");
    let bob = keyset();
    assert_eq!(
        verify_signature(
            &alice.ed25519_public(),
            SignatureContext::Share,
            b"messagE",
            &sig
        ),
        Err(CryptoError::SignatureInvalid)
    );
    assert_eq!(
        verify_signature(
            &bob.ed25519_public(),
            SignatureContext::Share,
            b"message",
            &sig
        ),
        Err(CryptoError::SignatureInvalid)
    );
    for i in 0..64 * 8 {
        let mut bad = sig;
        bad[i / 8] ^= 1 << (i % 8);
        assert!(
            verify_signature(
                &alice.ed25519_public(),
                SignatureContext::Share,
                b"message",
                &bad
            )
            .is_err(),
            "bit {i}"
        );
    }
}

#[test]
fn signatures_are_domain_separated() {
    // A plain Ed25519 signature over the bare message must not verify:
    // every signature covers `len(label) || label || message`.
    use ed25519_dalek::{Signer, SigningKey};
    let raw = SigningKey::from_bytes(&[9u8; 32]);
    let sig = raw.sign(b"message").to_bytes();
    assert_eq!(
        verify_signature(
            &raw.verifying_key().to_bytes(),
            SignatureContext::Share,
            b"message",
            &sig
        ),
        Err(CryptoError::SignatureInvalid)
    );
    let mut labelled = vec![13u8];
    labelled.extend_from_slice(b"pwm/share-sig");
    labelled.extend_from_slice(b"message");
    let sig = raw.sign(&labelled).to_bytes();
    assert_eq!(
        verify_signature(
            &raw.verifying_key().to_bytes(),
            SignatureContext::Share,
            b"message",
            &sig
        ),
        Ok(())
    );
}

#[test]
fn weak_public_keys_are_rejected() {
    let sig = keyset().sign(SignatureContext::Share, b"m");
    let mut identity = [0u8; 32];
    identity[0] = 1;
    for public in [[0u8; 32], identity] {
        assert_eq!(
            verify_signature(&public, SignatureContext::Share, b"m", &sig),
            Err(CryptoError::InvalidPublicKey)
        );
    }
}

// --- Vault shares ---

#[test]
fn aad_share_layout() {
    let aad = Aad::share(1, &VAULT, &[0xEE; 32], &[0xDD; 32]);
    let mut expected = vec![9u8];
    expected.extend_from_slice(b"pwm/share");
    expected.push(1);
    expected.extend_from_slice(&VAULT);
    expected.extend_from_slice(&[0xEE; 32]);
    expected.extend_from_slice(&[0xDD; 32]);
    assert_eq!(aad.as_bytes(), expected.as_slice());
}

#[test]
fn share_round_trip() {
    let (alice, bob) = (keyset(), keyset());
    let vault_key = SymmetricKey::generate().unwrap();
    let share = alice
        .share_vault_key(&vault_key, &VAULT, &bob.x25519_public())
        .unwrap();

    let bytes = share.to_bytes();
    assert_eq!(bytes[0], VaultShare::VERSION);
    assert_eq!(bytes.len(), 1 + 32 + 64 + Envelope::MIN_LEN + 32);
    assert_eq!(VaultShare::MIN_LEN, 1 + 32 + 64 + Envelope::MIN_LEN);

    let received = VaultShare::from_bytes(&bytes).unwrap();
    let key = bob
        .accept_vault_share(&received, &VAULT, &alice.ed25519_public())
        .unwrap();

    let aad = Aad::item(1, &VAULT, &[0xC1; 16], 1);
    let item = seal(&vault_key, b"shared item", &aad).unwrap();
    assert_eq!(
        open(&key, &item, &aad).unwrap().expose_secret(),
        b"shared item"
    );
}

#[test]
fn signature_covers_the_spec_message() {
    let (alice, bob) = (keyset(), keyset());
    let share = alice
        .share_vault_key(
            &SymmetricKey::generate().unwrap(),
            &VAULT,
            &bob.x25519_public(),
        )
        .unwrap();
    let bytes = share.to_bytes();
    let sig: [u8; 64] = bytes[33..97].try_into().unwrap();
    let msg = share_sig_message(&VAULT, &bob.x25519_public(), &bytes);
    assert_eq!(
        verify_signature(&alice.ed25519_public(), SignatureContext::Share, &msg, &sig),
        Ok(())
    );
}

#[test]
fn every_share_uses_a_fresh_ephemeral_key() {
    let (alice, bob) = (keyset(), keyset());
    let vault_key = SymmetricKey::generate().unwrap();
    let a = alice
        .share_vault_key(&vault_key, &VAULT, &bob.x25519_public())
        .unwrap()
        .to_bytes();
    let b = alice
        .share_vault_key(&vault_key, &VAULT, &bob.x25519_public())
        .unwrap()
        .to_bytes();
    assert_ne!(a[1..33], b[1..33]);
}

#[test]
fn share_is_bound_to_recipient_vault_and_sender() {
    let (alice, bob, carol) = (keyset(), keyset(), keyset());
    let share = alice
        .share_vault_key(
            &SymmetricKey::generate().unwrap(),
            &VAULT,
            &bob.x25519_public(),
        )
        .unwrap();
    // Someone else cannot accept it.
    assert_eq!(
        carol
            .accept_vault_share(&share, &VAULT, &alice.ed25519_public())
            .unwrap_err(),
        CryptoError::SignatureInvalid
    );
    // It cannot be moved to another vault.
    assert_eq!(
        bob.accept_vault_share(&share, &OTHER_VAULT, &alice.ed25519_public())
            .unwrap_err(),
        CryptoError::SignatureInvalid
    );
    // It is not accepted as coming from someone else.
    assert_eq!(
        bob.accept_vault_share(&share, &VAULT, &carol.ed25519_public())
            .unwrap_err(),
        CryptoError::SignatureInvalid
    );
}

#[test]
fn re_signing_as_another_sender_fails_decryption() {
    // Mallory replaces Alice's signature with her own. The signature now
    // verifies under Mallory's key, but the AAD binds Alice as sender.
    let (alice, bob, mallory) = (keyset(), keyset(), keyset());
    let share = alice
        .share_vault_key(
            &SymmetricKey::generate().unwrap(),
            &VAULT,
            &bob.x25519_public(),
        )
        .unwrap();
    let mut bytes = share.to_bytes();
    let msg = share_sig_message(&VAULT, &bob.x25519_public(), &bytes);
    let forged = mallory.sign(SignatureContext::Share, &msg);
    bytes[33..97].copy_from_slice(&forged);
    let forged_share = VaultShare::from_bytes(&bytes).unwrap();
    assert_eq!(
        bob.accept_vault_share(&forged_share, &VAULT, &mallory.ed25519_public())
            .unwrap_err(),
        CryptoError::DecryptionFailed
    );
}

#[test]
fn every_bit_flip_is_rejected() {
    let (alice, bob) = (keyset(), keyset());
    let bytes = alice
        .share_vault_key(
            &SymmetricKey::generate().unwrap(),
            &VAULT,
            &bob.x25519_public(),
        )
        .unwrap()
        .to_bytes();
    for i in 0..bytes.len() * 8 {
        let mut t = bytes.clone();
        t[i / 8] ^= 1 << (i % 8);
        let result = VaultShare::from_bytes(&t)
            .and_then(|s| bob.accept_vault_share(&s, &VAULT, &alice.ed25519_public()));
        assert!(result.is_err(), "bit {i} flip accepted");
    }
}

#[test]
fn low_order_recipient_key_is_rejected() {
    let alice = keyset();
    let vault_key = SymmetricKey::generate().unwrap();
    assert_eq!(
        alice
            .share_vault_key(&vault_key, &VAULT, &[0u8; 32])
            .unwrap_err(),
        CryptoError::InvalidPublicKey
    );
}

#[test]
fn low_order_ephemeral_key_is_rejected_after_signature_check() {
    // A malicious but correctly signing sender cannot force a known `dh`.
    let (mallory, bob) = (keyset(), keyset());
    let mut bytes = mallory
        .share_vault_key(
            &SymmetricKey::generate().unwrap(),
            &VAULT,
            &bob.x25519_public(),
        )
        .unwrap()
        .to_bytes();
    bytes[1..33].copy_from_slice(&[0u8; 32]);
    let msg = share_sig_message(&VAULT, &bob.x25519_public(), &bytes);
    let sig = mallory.sign(SignatureContext::Share, &msg);
    bytes[33..97].copy_from_slice(&sig);
    let share = VaultShare::from_bytes(&bytes).unwrap();
    assert_eq!(
        bob.accept_vault_share(&share, &VAULT, &mallory.ed25519_public())
            .unwrap_err(),
        CryptoError::InvalidPublicKey
    );
}

#[test]
fn decode_rejects_unknown_version_and_short_input() {
    let mut bytes = vec![1u8; VaultShare::MIN_LEN];
    bytes[0] = 2;
    assert_eq!(
        VaultShare::from_bytes(&bytes).unwrap_err(),
        CryptoError::UnsupportedVersion(2)
    );
    assert_eq!(
        VaultShare::from_bytes(&[1u8; VaultShare::MIN_LEN - 1]).unwrap_err(),
        CryptoError::Truncated
    );
    assert_eq!(
        VaultShare::from_bytes(&[]).unwrap_err(),
        CryptoError::Truncated
    );
}

#[test]
fn release_keys_sign_from_a_seed_and_labels_keep_purposes_apart() {
    let seed = pwm_crypto::Secret::new([7u8; 32]);
    let public = pwm_crypto::public_key_from_seed(&seed);
    let sig = pwm_crypto::sign_with_seed(&seed, SignatureContext::AutofillRules, b"rules");
    assert!(verify_signature(&public, SignatureContext::AutofillRules, b"rules", &sig).is_ok());
    // The same bytes are not a share signature, and a changed message fails.
    assert!(verify_signature(&public, SignatureContext::Share, b"rules", &sig).is_err());
    assert!(verify_signature(&public, SignatureContext::AutofillRules, b"rules!", &sig).is_err());
    // Deterministic, as Ed25519 is.
    assert_eq!(
        sig,
        pwm_crypto::sign_with_seed(&seed, SignatureContext::AutofillRules, b"rules")
    );
}
