//! Key hierarchy (docs/SECURITY.md): password normalization (D16), AUK and
//! auth secret (D13), account keyset create/unlock, vault key wrap/unwrap.

use pwm_crypto::{
    Aad, AccountKeyset, CryptoError, Envelope, Id, KdfParams, Secret, SecretKey, SymmetricKey,
    create_account, derive_account_keys, derive_password_key, hkdf_sha256, normalize_password,
    open, seal, unlock_account,
};

const ACCOUNT: Id = [0xA1; 16];
const OTHER_ACCOUNT: Id = [0xA2; 16];
const VAULT: Id = [0xB1; 16];
const SALT: [u8; 16] = [0x5A; 16];
const SK: [u8; 16] = [0x0F; 16];
const PARAMS: KdfParams = KdfParams::MOBILE_FLOOR;

fn secret_key(bytes: [u8; 16]) -> SecretKey {
    SecretKey::from_bytes(Secret::new(bytes))
}

// --- D16: password normalization ---

#[test]
fn normalization_unifies_composed_and_decomposed_accents() {
    let composed = normalize_password("caf\u{e9}").unwrap();
    let decomposed = normalize_password("cafe\u{301}").unwrap();
    assert_eq!(composed.expose_secret(), decomposed.expose_secret());
    assert_eq!(composed.expose_secret(), "caf\u{e9}");
}

#[test]
fn normalization_maps_unicode_spaces_to_ascii_space() {
    for space in [
        '\u{a0}', '\u{1680}', '\u{2000}', '\u{2005}', '\u{200a}', '\u{202f}', '\u{205f}',
        '\u{3000}',
    ] {
        let pw = normalize_password(&format!("a{space}b")).unwrap();
        assert_eq!(pw.expose_secret(), "a b", "U+{:04X}", space as u32);
    }
}

#[test]
fn normalization_keeps_case_and_does_not_trim() {
    assert_eq!(
        normalize_password("PassWord").unwrap().expose_secret(),
        "PassWord"
    );
    assert_eq!(normalize_password(" pw ").unwrap().expose_secret(), " pw ");
}

#[test]
fn normalization_rejects_empty_and_control_characters() {
    for pw in ["", "pw\n", "\tpw", "p\u{7}w", "pw\u{7f}", "p\u{85}w"] {
        assert_eq!(
            normalize_password(pw).unwrap_err(),
            CryptoError::PasswordRejected,
            "{pw:?}"
        );
    }
}

// --- D13: AUK and auth secret ---

#[test]
fn account_keys_follow_the_spec_composition() {
    let keys = derive_account_keys("hunter2", &secret_key(SK), &ACCOUNT, &SALT, &PARAMS).unwrap();

    let pw_key = derive_password_key(b"hunter2", &SALT, &PARAMS).unwrap();
    let sk_key = hkdf_sha256(&SK, &ACCOUNT, b"pwm/sk/v1");
    let mut mixed = [0u8; 32];
    for (m, (p, s)) in mixed
        .iter_mut()
        .zip(pw_key.expose_secret().iter().zip(sk_key.expose_secret()))
    {
        *m = p ^ s;
    }
    let expected_auk = hkdf_sha256(&mixed, &ACCOUNT, b"pwm/auk/v1");
    let expected_auth = hkdf_sha256(&mixed, &ACCOUNT, b"pwm/auth/v1");

    assert_eq!(
        keys.auth_secret().expose_secret(),
        expected_auth.expose_secret()
    );
    // The AUK is not exposable; prove equality by decrypting across the two.
    let aad = Aad::keyset(1, &ACCOUNT);
    let env = seal(keys.auk(), b"probe", &aad).unwrap();
    let expected = SymmetricKey::from_secret(expected_auk);
    assert_eq!(
        open(&expected, &env, &aad).unwrap().expose_secret(),
        b"probe"
    );
}

#[test]
fn auk_and_auth_secret_are_independent() {
    let keys = derive_account_keys("hunter2", &secret_key(SK), &ACCOUNT, &SALT, &PARAMS).unwrap();
    let aad = Aad::keyset(1, &ACCOUNT);
    let env = seal(keys.auk(), b"probe", &aad).unwrap();
    let as_key = SymmetricKey::from_secret(Secret::new(*keys.auth_secret().expose_secret()));
    assert!(open(&as_key, &env, &aad).is_err());
}

#[test]
fn every_input_changes_the_keys() {
    let base = derive_account_keys("hunter2", &secret_key(SK), &ACCOUNT, &SALT, &PARAMS).unwrap();
    let variants = [
        derive_account_keys("hunter3", &secret_key(SK), &ACCOUNT, &SALT, &PARAMS).unwrap(),
        derive_account_keys("hunter2", &secret_key([0x10; 16]), &ACCOUNT, &SALT, &PARAMS).unwrap(),
        derive_account_keys("hunter2", &secret_key(SK), &OTHER_ACCOUNT, &SALT, &PARAMS).unwrap(),
        derive_account_keys("hunter2", &secret_key(SK), &ACCOUNT, &[0x5B; 16], &PARAMS).unwrap(),
    ];
    for v in &variants {
        assert_ne!(
            v.auth_secret().expose_secret(),
            base.auth_secret().expose_secret()
        );
    }
}

#[test]
fn rejected_password_and_params_fail_derivation() {
    assert_eq!(
        derive_account_keys("", &secret_key(SK), &ACCOUNT, &SALT, &PARAMS).unwrap_err(),
        CryptoError::PasswordRejected
    );
    let weak = KdfParams {
        memory_kib: 1024,
        iterations: 3,
        parallelism: 1,
    };
    assert_eq!(
        derive_account_keys("pw", &secret_key(SK), &ACCOUNT, &SALT, &weak).unwrap_err(),
        CryptoError::KdfParamsRejected
    );
}

// --- Account keyset ---

#[test]
fn keyset_round_trip_preserves_all_keys() {
    let auk = SymmetricKey::generate().unwrap();
    let keyset = AccountKeyset::generate().unwrap();
    let sealed = keyset.seal(&auk, &ACCOUNT).unwrap();
    // 1 version byte + 3 keys of 32 bytes inside the envelope.
    assert_eq!(sealed.to_bytes().len(), Envelope::MIN_LEN + 97);

    let reopened = AccountKeyset::open(&sealed, &auk, &ACCOUNT).unwrap();
    assert_eq!(reopened.x25519_public(), keyset.x25519_public());
    assert_eq!(reopened.ed25519_public(), keyset.ed25519_public());

    let vault_key = SymmetricKey::generate().unwrap();
    let wrapped = keyset.wrap_vault_key(&vault_key, &VAULT).unwrap();
    assert!(reopened.unwrap_vault_key(&wrapped, &VAULT).is_ok());
}

#[test]
fn keysets_are_random() {
    let a = AccountKeyset::generate().unwrap();
    let b = AccountKeyset::generate().unwrap();
    assert_ne!(a.x25519_public(), b.x25519_public());
    assert_ne!(a.ed25519_public(), b.ed25519_public());
}

#[test]
fn keyset_is_bound_to_auk_and_account() {
    let auk = SymmetricKey::generate().unwrap();
    let sealed = AccountKeyset::generate()
        .unwrap()
        .seal(&auk, &ACCOUNT)
        .unwrap();
    let wrong_auk = SymmetricKey::generate().unwrap();
    assert_eq!(
        AccountKeyset::open(&sealed, &wrong_auk, &ACCOUNT).unwrap_err(),
        CryptoError::DecryptionFailed
    );
    assert_eq!(
        AccountKeyset::open(&sealed, &auk, &OTHER_ACCOUNT).unwrap_err(),
        CryptoError::DecryptionFailed
    );
}

#[test]
fn keyset_debug_is_redacted() {
    let out = format!("{:?}", AccountKeyset::generate().unwrap());
    assert!(out.contains("REDACTED"), "{out}");
}

// --- Vault keys ---

#[test]
fn vault_key_round_trip() {
    let keyset = AccountKeyset::generate().unwrap();
    let vault_key = SymmetricKey::generate().unwrap();
    let wrapped = keyset.wrap_vault_key(&vault_key, &VAULT).unwrap();
    assert_eq!(wrapped.to_bytes().len(), Envelope::MIN_LEN + 32);
    let unwrapped = keyset.unwrap_vault_key(&wrapped, &VAULT).unwrap();

    let aad = Aad::item(1, &VAULT, &[0xC1; 16], 1);
    let item = seal(&vault_key, b"item", &aad).unwrap();
    assert_eq!(
        open(&unwrapped, &item, &aad).unwrap().expose_secret(),
        b"item"
    );
}

#[test]
fn vault_key_is_bound_to_keyset_and_vault() {
    let keyset = AccountKeyset::generate().unwrap();
    let wrapped = keyset
        .wrap_vault_key(&SymmetricKey::generate().unwrap(), &VAULT)
        .unwrap();
    assert_eq!(
        keyset.unwrap_vault_key(&wrapped, &[0xB2; 16]).unwrap_err(),
        CryptoError::DecryptionFailed
    );
    let other = AccountKeyset::generate().unwrap();
    assert_eq!(
        other.unwrap_vault_key(&wrapped, &VAULT).unwrap_err(),
        CryptoError::DecryptionFailed
    );
}

#[test]
fn keyset_envelope_cannot_be_used_as_vault_key() {
    let auk = SymmetricKey::generate().unwrap();
    let keyset = AccountKeyset::generate().unwrap();
    let sealed = keyset.seal(&auk, &ACCOUNT).unwrap();
    assert!(keyset.unwrap_vault_key(&sealed, &ACCOUNT).is_err());
}

// --- Create and unlock ---

#[test]
fn create_then_unlock() {
    let account = create_account("correct horse battery", &ACCOUNT, PARAMS).unwrap();
    assert_eq!(account.params, PARAMS);
    let unlocked = unlock_account(
        "correct horse battery",
        &account.secret_key,
        &ACCOUNT,
        &account.salt,
        &account.params,
        &account.sealed_keyset,
    )
    .unwrap();
    assert_eq!(unlocked.x25519_public(), account.keyset.x25519_public());
    assert_eq!(unlocked.ed25519_public(), account.keyset.ed25519_public());

    let again = derive_account_keys(
        "correct horse battery",
        &account.secret_key,
        &ACCOUNT,
        &account.salt,
        &account.params,
    )
    .unwrap();
    assert_eq!(
        again.auth_secret().expose_secret(),
        account.auth_secret.expose_secret()
    );
}

#[test]
fn unlock_accepts_differently_encoded_but_equal_password() {
    let account = create_account("caf\u{e9}\u{a0}pw", &ACCOUNT, PARAMS).unwrap();
    assert!(
        unlock_account(
            "cafe\u{301} pw",
            &account.secret_key,
            &ACCOUNT,
            &account.salt,
            &account.params,
            &account.sealed_keyset,
        )
        .is_ok()
    );
}

#[test]
fn unlock_fails_with_any_wrong_input() {
    let account = create_account("correct horse battery", &ACCOUNT, PARAMS).unwrap();
    let wrong_sk = SecretKey::generate().unwrap();
    let attempts = [
        unlock_account(
            "correct horse batterY",
            &account.secret_key,
            &ACCOUNT,
            &account.salt,
            &account.params,
            &account.sealed_keyset,
        ),
        unlock_account(
            "correct horse battery",
            &wrong_sk,
            &ACCOUNT,
            &account.salt,
            &account.params,
            &account.sealed_keyset,
        ),
        unlock_account(
            "correct horse battery",
            &account.secret_key,
            &OTHER_ACCOUNT,
            &account.salt,
            &account.params,
            &account.sealed_keyset,
        ),
        unlock_account(
            "correct horse battery",
            &account.secret_key,
            &ACCOUNT,
            &[0; 16],
            &account.params,
            &account.sealed_keyset,
        ),
    ];
    for result in attempts {
        assert_eq!(result.unwrap_err(), CryptoError::DecryptionFailed);
    }
}

#[test]
fn new_accounts_get_fresh_secrets() {
    let a = create_account("pw", &ACCOUNT, PARAMS).unwrap();
    let b = create_account("pw", &ACCOUNT, PARAMS).unwrap();
    assert_ne!(a.salt, b.salt);
    assert_ne!(
        a.secret_key.to_text().expose_secret(),
        b.secret_key.to_text().expose_secret()
    );
    assert_ne!(a.auth_secret.expose_secret(), b.auth_secret.expose_secret());
}
