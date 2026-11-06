//! Windows Hello unlock (decision D68). Windows Hello's key can only sign,
//! so the biometric unlock key (D45) is sealed under a key derived from a
//! signature: `HKDF-SHA256(ikm = signature, salt = challenge,
//! info = "pwm/windows-hello/v1")`. The challenge is 32 random bytes kept
//! on the device; the signature is made by the TPM key after a Windows
//! Hello gesture and is never stored. RSA PKCS#1 v1.5 signatures are
//! deterministic, so the same key signing the same challenge gives the same
//! wrapping key.
//!
//! The browser helper (decision D69) keeps the extension's key K the same
//! way, with its own label (`pwm/browser-unlock/windows-hello/v1`) and the
//! calling extension in the AAD, so the two never stand in for each other.

use crate::account::random_32;
use crate::{Aad, CryptoError, Envelope, Id, Secret, SymmetricKey, hkdf_sha256, open, seal};

/// The format of the sealed unlock key.
pub const WINDOWS_HELLO_VERSION: u8 = 1;
const INFO: &[u8] = b"pwm/windows-hello/v1";
/// The format of the browser helper's sealed key (D69).
pub const BROWSER_HELLO_VERSION: u8 = 1;
const BROWSER_INFO: &[u8] = b"pwm/browser-unlock/windows-hello/v1";
/// Shorter than any RSA signature Windows Hello makes (2048 bits).
const MIN_SIGNATURE_LEN: usize = 64;

/// A new random challenge for Windows Hello to sign.
pub fn windows_hello_challenge() -> Result<Secret<[u8; 32]>, CryptoError> {
    random_32()
}

fn wrapping_key(
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    info: &[u8],
) -> Result<SymmetricKey, CryptoError> {
    if signature.len() < MIN_SIGNATURE_LEN {
        return Err(CryptoError::InvalidMessage);
    }
    Ok(SymmetricKey::from_secret(hkdf_sha256(
        signature,
        challenge.expose_secret(),
        info,
    )))
}

fn key_32(plaintext: &Secret<Vec<u8>>) -> Result<Secret<[u8; 32]>, CryptoError> {
    let bytes: &[u8] = plaintext.expose_secret();
    if bytes.len() != 32 {
        return Err(CryptoError::DecryptionFailed);
    }
    Ok(Secret::<[u8; 32]>::new_with(|k| k.copy_from_slice(bytes)))
}

/// Seals the 32-byte biometric unlock key under the signature's key, bound
/// to the account and the device (AAD `pwm/windows-hello`).
pub fn seal_windows_hello(
    unlock_key: &Secret<[u8; 32]>,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    account_id: &Id,
    device_id: &Id,
) -> Result<Envelope, CryptoError> {
    let key = wrapping_key(signature, challenge, INFO)?;
    seal(
        &key,
        unlock_key.expose_secret(),
        &Aad::windows_hello(WINDOWS_HELLO_VERSION, account_id, device_id),
    )
}

/// Opens [`seal_windows_hello`]'s envelope with a new signature over the
/// same challenge. Any other signature, challenge, account or device fails.
pub fn open_windows_hello(
    envelope: &Envelope,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    account_id: &Id,
    device_id: &Id,
) -> Result<Secret<[u8; 32]>, CryptoError> {
    let key = wrapping_key(signature, challenge, INFO)?;
    let plaintext = open(
        &key,
        envelope,
        &Aad::windows_hello(WINDOWS_HELLO_VERSION, account_id, device_id),
    )?;
    key_32(&plaintext)
}

/// Seals the extension's 32-byte key K (D48 §5) for the Windows browser
/// helper (D69), under a Windows Hello signature's key, bound to the caller
/// (the extension's origin or Firefox add-on id), the account and the device.
pub fn seal_browser_hello(
    browser_key: &Secret<[u8; 32]>,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    caller: &str,
    account_id: &Id,
    device_id: &Id,
) -> Result<Envelope, CryptoError> {
    let aad = Aad::browser_hello(BROWSER_HELLO_VERSION, caller, account_id, device_id)?;
    let key = wrapping_key(signature, challenge, BROWSER_INFO)?;
    seal(&key, browser_key.expose_secret(), &aad)
}

/// Opens [`seal_browser_hello`]'s envelope with a new signature over the
/// same challenge, for the same caller, account and device.
pub fn open_browser_hello(
    envelope: &Envelope,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    caller: &str,
    account_id: &Id,
    device_id: &Id,
) -> Result<Secret<[u8; 32]>, CryptoError> {
    let aad = Aad::browser_hello(BROWSER_HELLO_VERSION, caller, account_id, device_id)?;
    let key = wrapping_key(signature, challenge, BROWSER_INFO)?;
    key_32(&open(&key, envelope, &aad)?)
}
