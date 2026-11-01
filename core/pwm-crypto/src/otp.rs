//! HMAC-based one-time password step, RFC 4226 section 5.3 (decision D23).

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use sha2::{Sha256, Sha512};

/// Hash used by a TOTP/HOTP secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OtpAlgorithm {
    #[default]
    Sha1,
    Sha256,
    Sha512,
}

/// HMAC of the big-endian `counter` under `key`, reduced by dynamic
/// truncation to a 31-bit value. Callers take it modulo `10^digits` (or map
/// it to another alphabet).
pub fn hotp_value(algorithm: OtpAlgorithm, key: &[u8], counter: u64) -> u32 {
    let msg = counter.to_be_bytes();
    match algorithm {
        OtpAlgorithm::Sha1 => truncate(&mac::<Hmac<Sha1>>(key, &msg)),
        OtpAlgorithm::Sha256 => truncate(&mac::<Hmac<Sha256>>(key, &msg)),
        OtpAlgorithm::Sha512 => truncate(&mac::<Hmac<Sha512>>(key, &msg)),
    }
}

fn mac<M: Mac + KeyInit>(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut mac = <M as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

fn truncate(digest: &[u8]) -> u32 {
    let offset = usize::from(digest[digest.len() - 1] & 0x0F);
    let bytes: [u8; 4] = digest[offset..offset + 4]
        .try_into()
        .expect("offset + 4 is within every SHA digest");
    u32::from_be_bytes(bytes) & 0x7FFF_FFFF
}
