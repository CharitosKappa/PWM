//! Crypto primitives wrappers, KDF, key hierarchy and `Secret<T>`. See `docs/SECURITY.md`.
#![forbid(unsafe_code)]

mod aad;
mod aead;
mod error;
mod secret;

pub use aad::{Aad, Id};
pub use aead::{Envelope, SymmetricKey, open, seal};
pub use error::CryptoError;
pub use secret::Secret;
