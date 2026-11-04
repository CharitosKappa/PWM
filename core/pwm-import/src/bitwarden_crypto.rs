//! Opening Bitwarden's password-protected export (D51 §4), from Bitwarden's
//! published description of the format:
//!
//! - The key: PBKDF2-SHA256(password, salt, iterations) (`kdfType` 0), or
//!   Argon2id(password, SHA-256(salt), memory, iterations, parallelism)
//!   (`kdfType` 1), 32 bytes.
//! - Stretched with HKDF-Expand-SHA256 into an encryption key (info "enc")
//!   and a MAC key (info "mac").
//! - Each value is an "EncString" of type 2: `2.<iv>|<ciphertext>|<mac>` in
//!   standard base64, AES-256-CBC with PKCS#7 padding, and HMAC-SHA256 over
//!   `iv || ciphertext`, checked before decrypting.
//!
//! These primitives open this format and nothing else; our own formats use
//! the vetted AEAD in `pwm-crypto`.

use aes::Aes256;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockModeDecrypt, KeyIvInit};
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{ImportError, json};

/// Bounds on what a file may ask for, so a crafted one can't stall a device.
const PBKDF2_ITERATIONS: std::ops::RangeInclusive<u32> = 5_000..=2_000_000;
const ARGON2_MEMORY_MIB: std::ops::RangeInclusive<u32> = 16..=1024;
const ARGON2_ITERATIONS: std::ops::RangeInclusive<u32> = 1..=10;
const ARGON2_PARALLELISM: std::ops::RangeInclusive<u32> = 1..=16;

struct Keys {
    enc: Zeroizing<[u8; 32]>,
    mac: Zeroizing<[u8; 32]>,
}

/// The export's inner JSON.
pub fn open(doc: &Value, password: &str) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    let keys = derive(doc, password)?;
    // The validation value proves the password before the data is touched.
    let check = json::s(doc, "encKeyValidation_DO_NOT_EDIT");
    if !check.is_empty() {
        decrypt(&keys, &check)?;
    }
    decrypt(&keys, &json::s(doc, "data"))
}

fn u32_of(doc: &Value, key: &str) -> Option<u32> {
    json::int(doc, key).and_then(|n| u32::try_from(n).ok())
}

fn derive(doc: &Value, password: &str) -> Result<Keys, ImportError> {
    let salt = json::s(doc, "salt");
    if salt.is_empty() {
        return Err(ImportError::Unreadable);
    }
    let iterations = u32_of(doc, "kdfIterations").ok_or(ImportError::Unreadable)?;
    let mut key = Zeroizing::new([0u8; 32]);
    match json::int(doc, "kdfType").unwrap_or(0) {
        0 => {
            if !PBKDF2_ITERATIONS.contains(&iterations) {
                return Err(ImportError::Unreadable);
            }
            pbkdf2::pbkdf2_hmac::<Sha256>(
                password.as_bytes(),
                salt.as_bytes(),
                iterations,
                &mut *key,
            );
        }
        1 => {
            let memory = u32_of(doc, "kdfMemory").ok_or(ImportError::Unreadable)?;
            let parallelism = u32_of(doc, "kdfParallelism").ok_or(ImportError::Unreadable)?;
            if !ARGON2_MEMORY_MIB.contains(&memory)
                || !ARGON2_ITERATIONS.contains(&iterations)
                || !ARGON2_PARALLELISM.contains(&parallelism)
            {
                return Err(ImportError::Unreadable);
            }
            let params = argon2::Params::new(memory * 1024, iterations, parallelism, Some(32))
                .map_err(|_| ImportError::Unreadable)?;
            let argon =
                argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
            let salt = Sha256::digest(salt.as_bytes());
            argon
                .hash_password_into(password.as_bytes(), &salt, &mut *key)
                .map_err(|_| ImportError::Unreadable)?;
        }
        _ => return Err(ImportError::Unreadable),
    }
    let hkdf = Hkdf::<Sha256>::from_prk(&*key).map_err(|_| ImportError::Unreadable)?;
    let mut enc = Zeroizing::new([0u8; 32]);
    let mut mac = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"enc", &mut *enc)
        .map_err(|_| ImportError::Unreadable)?;
    hkdf.expand(b"mac", &mut *mac)
        .map_err(|_| ImportError::Unreadable)?;
    Ok(Keys { enc, mac })
}

/// One type-2 EncString. A MAC that doesn't match is a wrong password.
fn decrypt(keys: &Keys, enc_string: &str) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    let body = enc_string
        .strip_prefix("2.")
        .ok_or(ImportError::Unreadable)?;
    let mut parts = body.split('|');
    let (Some(iv), Some(ct), Some(tag), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ImportError::Unreadable);
    };
    let decode = |s: &str| {
        STANDARD
            .decode(s.trim())
            .map_err(|_| ImportError::Unreadable)
    };
    let (iv, mut ct, tag) = (decode(iv)?, Zeroizing::new(decode(ct)?), decode(tag)?);
    if iv.len() != 16 {
        return Err(ImportError::Unreadable);
    }
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&*keys.mac)
        .map_err(|_| ImportError::Unreadable)?;
    mac.update(&iv);
    mac.update(&ct);
    mac.verify_slice(&tag)
        .map_err(|_| ImportError::WrongPassword)?;
    let cipher = cbc::Decryptor::<Aes256>::new_from_slices(&*keys.enc, &iv)
        .map_err(|_| ImportError::Unreadable)?;
    let plain = cipher
        .decrypt_padded::<Pkcs7>(&mut ct)
        .map_err(|_| ImportError::Unreadable)?;
    Ok(Zeroizing::new(plain.to_vec()))
}
