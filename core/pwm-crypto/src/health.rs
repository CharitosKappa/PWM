//! Hashes for the health report (decision D22).

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha1::{Digest, Sha1};
use sha2::Sha256;

use crate::{CryptoError, Secret};

/// SHA-1 as required by the HIBP range API. Used for nothing else: SHA-1 is
/// not collision resistant, and HIBP only needs a shared, unkeyed hash.
pub fn hibp_sha1(data: &[u8]) -> Secret<[u8; 20]> {
    Secret::<[u8; 20]>::new_with(|out| out.copy_from_slice(&Sha1::digest(data)))
}

/// Finds reused passwords without keeping a crackable table: HMAC-SHA256
/// under a random key made per report run and zeroized when dropped.
pub struct ReuseHasher {
    key: Secret<[u8; 32]>,
}

impl ReuseHasher {
    pub fn new() -> Result<Self, CryptoError> {
        let mut ok = false;
        let key = Secret::<[u8; 32]>::new_with(|k| ok = getrandom::fill(k).is_ok());
        if !ok {
            return Err(CryptoError::Rng);
        }
        Ok(Self { key })
    }

    pub fn hash(&self, data: &[u8]) -> [u8; 32] {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.key.expose_secret())
            .expect("HMAC accepts any key length");
        mac.update(data);
        mac.finalize().into_bytes().into()
    }
}

impl fmt::Debug for ReuseHasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReuseHasher([REDACTED])")
    }
}
