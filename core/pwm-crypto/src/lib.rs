//! Crypto primitives wrappers, KDF, key hierarchy and `Secret<T>`. See `docs/SECURITY.md`.
#![forbid(unsafe_code)]

mod aad;
mod account;
mod aead;
mod error;
mod hkdf;
mod kdf;
mod password;
mod secret;
mod secret_key;
mod share;

pub use aad::{Aad, Id, generate_id};
pub use account::{
    AccountKeys, AccountKeyset, NewAccount, create_account, derive_account_keys, unlock_account,
};
pub use aead::{Envelope, SymmetricKey, open, seal};
pub use error::CryptoError;
pub use hkdf::hkdf_sha256;
pub use kdf::{KdfParams, derive_password_key, generate_salt};
pub use password::normalize_password;
pub use secret::Secret;
pub use secret_key::SecretKey;
pub use share::{SignatureContext, VaultShare, verify_signature};
