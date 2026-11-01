//! Crypto primitives wrappers, KDF, key hierarchy and `Secret<T>`. See `docs/SECURITY.md`.
#![forbid(unsafe_code)]

mod aad;
mod aead;
mod error;
mod hkdf;
mod kdf;
mod secret;
mod secret_key;

pub use aad::{Aad, Id};
pub use aead::{Envelope, SymmetricKey, open, seal};
pub use error::CryptoError;
pub use hkdf::hkdf_sha256;
pub use kdf::{KdfParams, derive_password_key, generate_salt};
pub use secret::Secret;
pub use secret_key::SecretKey;
