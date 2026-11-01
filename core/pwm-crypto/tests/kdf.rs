//! Argon2id password KDF with stored, validated params (D12, D15).

use pwm_crypto::{CryptoError, KdfParams, derive_password_key, generate_salt};

const SALT: [u8; 16] = [0x5A; 16];

fn params(memory_kib: u32, iterations: u32, parallelism: u32) -> KdfParams {
    KdfParams {
        memory_kib,
        iterations,
        parallelism,
    }
}

#[test]
fn spec_params() {
    assert_eq!(KdfParams::DEFAULT, params(64 * 1024, 3, 4));
    assert_eq!(KdfParams::MOBILE_FLOOR, params(32 * 1024, 4, 4));
}

#[test]
fn spec_params_are_accepted() {
    assert_eq!(KdfParams::DEFAULT.validate(), Ok(()));
    assert_eq!(KdfParams::MOBILE_FLOOR.validate(), Ok(()));
    assert_eq!(params(128 * 1024, 5, 8).validate(), Ok(()));
}

#[test]
fn weak_or_absurd_params_are_rejected() {
    let rejected = [
        params(16 * 1024, 8, 4),       // below the memory floor
        params(32 * 1024, 3, 4),       // less total work than the mobile floor
        params(64 * 1024, 2, 4),       // too few passes
        params(64 * 1024, 3, 0),       // no lanes
        params(64 * 1024, 3, 17),      // too many lanes
        params(2 * 1024 * 1024, 3, 4), // 2 GiB: refuse to allocate
        params(64 * 1024, 100, 4),     // unreasonably slow
    ];
    for p in rejected {
        assert_eq!(p.validate(), Err(CryptoError::KdfParamsRejected), "{p:?}");
        assert_eq!(
            derive_password_key(b"pw", &SALT, &p).unwrap_err(),
            CryptoError::KdfParamsRejected,
            "{p:?}"
        );
    }
}

#[test]
fn derivation_is_deterministic_and_input_sensitive() {
    let p = KdfParams::MOBILE_FLOOR;
    let a = derive_password_key(b"correct horse", &SALT, &p).unwrap();
    let b = derive_password_key(b"correct horse", &SALT, &p).unwrap();
    assert_eq!(a.expose_secret(), b.expose_secret());

    let other_pw = derive_password_key(b"correct horsf", &SALT, &p).unwrap();
    let other_salt = derive_password_key(b"correct horse", &[0x5B; 16], &p).unwrap();
    let other_params =
        derive_password_key(b"correct horse", &SALT, &params(32 * 1024, 5, 4)).unwrap();
    for other in [&other_pw, &other_salt, &other_params] {
        assert_ne!(a.expose_secret(), other.expose_secret());
    }
}

#[test]
fn default_params_derive() {
    let key = derive_password_key(b"pw", &SALT, &KdfParams::DEFAULT).unwrap();
    assert_ne!(key.expose_secret(), &[0u8; 32]);
}

#[test]
fn salts_are_random() {
    assert_ne!(generate_salt().unwrap(), generate_salt().unwrap());
}
