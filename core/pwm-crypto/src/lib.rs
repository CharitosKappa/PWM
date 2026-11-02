//! Crypto primitives wrappers, KDF, key hierarchy and `Secret<T>`. See `docs/SECURITY.md`.
#![forbid(unsafe_code)]

mod aad;
mod account;
mod aead;
mod error;
mod export;
mod health;
mod hkdf;
mod kdf;
mod opaque;
mod otp;
mod password;
mod random;
mod secret;
mod secret_key;
mod share;
mod token;

pub use aad::{Aad, Id, generate_id};
pub use account::{
    AccountKeys, AccountKeyset, NewAccount, create_account, derive_account_keys, random_seed,
    unlock_account,
};
pub use aead::{Envelope, SymmetricKey, open, seal};
pub use error::CryptoError;
pub use export::{EXPORT_MAGIC, ExportHeader, open_export, seal_export};
pub use health::{ReuseHasher, hibp_sha1};
pub use hkdf::hkdf_sha256;
pub use kdf::{KdfParams, derive_password_key, generate_salt};
pub use opaque::{OpaqueLogin, OpaqueRegistration, ServerSecrets, opaque_record};
pub use otp::{OtpAlgorithm, hotp_value};
pub use password::normalize_password;
pub use random::random_index;
pub use secret::Secret;
pub use secret_key::SecretKey;
pub use share::{
    SignatureContext, VaultShare, public_key_from_seed, sign_with_seed, verify_signature,
};
pub use token::{TokenKind, generate_token, token_hash};
