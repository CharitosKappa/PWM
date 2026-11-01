//! HKDF-SHA256 with 32-byte output (decision D13).

use hkdf::Hkdf;
use sha2::Sha256;

use crate::Secret;

/// `HKDF-SHA256(ikm, salt, info)`, 32 bytes. An empty `salt` is the RFC 5869
/// default (all zeros).
pub fn hkdf_sha256(ikm: &[u8], salt: &[u8], info: &[u8]) -> Secret<[u8; 32]> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), ikm);
    Secret::<[u8; 32]>::new_with(|okm| {
        hkdf.expand(info, okm)
            .expect("32 bytes is within the HKDF-SHA256 output limit");
    })
}
