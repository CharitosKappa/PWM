//! The checks on the billing providers' webhooks (D6 §3): Stripe signs each
//! one with HMAC-SHA256 under the endpoint's secret, and Play's push URL
//! carries a secret of ours. Both comparisons take the same time whatever
//! the input.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// `true` when `signature_hex` (64 hex digits, either case) is
/// HMAC-SHA256 of `payload` under `secret`.
pub fn webhook_signature_valid(secret: &[u8], payload: &[u8], signature_hex: &str) -> bool {
    let Some(signature) = unhex_32(signature_hex) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(payload);
    mac.verify_slice(&signature).is_ok()
}

/// `true` when `given` is `expected`, compared through their SHA-256 so the
/// time taken says nothing about either. An empty `expected` never matches.
pub fn secret_matches(given: &[u8], expected: &[u8]) -> bool {
    if expected.is_empty() {
        return false;
    }
    let a = Sha256::digest(given);
    let b = Sha256::digest(expected);
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn unhex_32(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let digit = |b: u8| (b as char).to_digit(16);
    for (slot, [high, low]) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *slot = u8::try_from(digit(*high)? * 16 + digit(*low)?).ok()?;
    }
    Some(out)
}
