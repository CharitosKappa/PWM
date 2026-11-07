//! Unlock with a platform key that answers a challenge: Windows Hello
//! (decision D68) and the Linux key holder behind polkit (decision D71).
//!
//! Windows Hello's key can only sign, so the biometric unlock key (D45) is
//! sealed under a key derived from a signature: `HKDF-SHA256(ikm =
//! signature, salt = challenge, info = "pwm/windows-hello/v1")`. The
//! challenge is 32 random bytes kept on the device; the signature is made by
//! the TPM key after a Windows Hello gesture and is never stored. RSA
//! PKCS#1 v1.5 signatures are deterministic, so the same key signing the
//! same challenge gives the same wrapping key.
//!
//! On Linux the key holder answers with [`linux_polkit_response`] after
//! polkit's dialog, from a secret only root can read, and the same
//! construction runs with Linux's labels (`pwm/linux-polkit/v1`).
//!
//! The browser helper (decisions D69, D71) keeps the extension's key K the
//! same way, with its own labels and the calling extension in the AAD, so a
//! desktop key and a browser key never stand in for each other.

use crate::account::random_32;
use crate::{Aad, CryptoError, Envelope, Id, Secret, SymmetricKey, hkdf_sha256, open, seal};

/// The format of the sealed unlock key.
pub const WINDOWS_HELLO_VERSION: u8 = 1;
/// The format of the browser helper's sealed key (D69).
pub const BROWSER_HELLO_VERSION: u8 = 1;
/// The key holder's answer: `HKDF-SHA256(S, challenge, this)` (D71).
const LINUX_RESPONSE_INFO: &[u8] = b"pwm/linux-polkit/response/v1";

/// Which platform key answered the challenge. Each has its own labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signer {
    /// A Windows Hello key's RSA signature (D68, D69).
    WindowsHello,
    /// The Linux key holder's 32-byte answer, behind polkit (D71).
    LinuxPolkit,
}

impl Signer {
    fn info(self) -> &'static [u8] {
        match self {
            Self::WindowsHello => b"pwm/windows-hello/v1",
            Self::LinuxPolkit => b"pwm/linux-polkit/v1",
        }
    }

    fn browser_info(self) -> &'static [u8] {
        match self {
            Self::WindowsHello => b"pwm/browser-unlock/windows-hello/v1",
            Self::LinuxPolkit => b"pwm/browser-unlock/linux-polkit/v1",
        }
    }

    /// Windows Hello signs with RSA keys of 2048 bits or more; the key
    /// holder's answer is always 32 bytes.
    fn accepts(self, answer: &[u8]) -> bool {
        match self {
            Self::WindowsHello => answer.len() >= 64,
            Self::LinuxPolkit => answer.len() == 32,
        }
    }
}

/// A new random challenge for the platform key to answer.
pub fn windows_hello_challenge() -> Result<Secret<[u8; 32]>, CryptoError> {
    random_32()
}

/// A new random secret for the Linux key holder to keep (D71).
pub fn linux_polkit_secret() -> Result<Secret<[u8; 32]>, CryptoError> {
    random_32()
}

/// The Linux key holder's answer to a challenge (D71): `HKDF-SHA256(ikm =
/// secret, salt = challenge, info = "pwm/linux-polkit/response/v1")`.
pub fn linux_polkit_response(secret: &Secret<[u8; 32]>, challenge: &[u8; 32]) -> Secret<[u8; 32]> {
    hkdf_sha256(secret.expose_secret(), challenge, LINUX_RESPONSE_INFO)
}

fn wrapping_key(
    signer: Signer,
    answer: &[u8],
    challenge: &Secret<[u8; 32]>,
    info: &[u8],
) -> Result<SymmetricKey, CryptoError> {
    if !signer.accepts(answer) {
        return Err(CryptoError::InvalidMessage);
    }
    Ok(SymmetricKey::from_secret(hkdf_sha256(
        answer,
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

/// Seals the 32-byte biometric unlock key under the platform key's answer,
/// bound to the account and the device ([`Aad::signed_unlock`]).
pub fn seal_signed_unlock(
    signer: Signer,
    unlock_key: &Secret<[u8; 32]>,
    answer: &[u8],
    challenge: &Secret<[u8; 32]>,
    account_id: &Id,
    device_id: &Id,
) -> Result<Envelope, CryptoError> {
    let key = wrapping_key(signer, answer, challenge, signer.info())?;
    seal(
        &key,
        unlock_key.expose_secret(),
        &Aad::signed_unlock(signer, WINDOWS_HELLO_VERSION, account_id, device_id),
    )
}

/// Opens [`seal_signed_unlock`]'s envelope with a new answer to the same
/// challenge. Any other answer, challenge, account, device or platform fails.
pub fn open_signed_unlock(
    signer: Signer,
    envelope: &Envelope,
    answer: &[u8],
    challenge: &Secret<[u8; 32]>,
    account_id: &Id,
    device_id: &Id,
) -> Result<Secret<[u8; 32]>, CryptoError> {
    let key = wrapping_key(signer, answer, challenge, signer.info())?;
    let plaintext = open(
        &key,
        envelope,
        &Aad::signed_unlock(signer, WINDOWS_HELLO_VERSION, account_id, device_id),
    )?;
    key_32(&plaintext)
}

/// Seals the extension's 32-byte key K (D48 §5) for the browser helper
/// (D69, D71), under the platform key's answer, bound to the caller (the
/// extension's origin or Firefox add-on id), the account and the device.
pub fn seal_signed_browser(
    signer: Signer,
    browser_key: &Secret<[u8; 32]>,
    answer: &[u8],
    challenge: &Secret<[u8; 32]>,
    caller: &str,
    account_id: &Id,
    device_id: &Id,
) -> Result<Envelope, CryptoError> {
    let aad = Aad::signed_browser(signer, BROWSER_HELLO_VERSION, caller, account_id, device_id)?;
    let key = wrapping_key(signer, answer, challenge, signer.browser_info())?;
    seal(&key, browser_key.expose_secret(), &aad)
}

/// Opens [`seal_signed_browser`]'s envelope with a new answer to the same
/// challenge, for the same platform, caller, account and device.
pub fn open_signed_browser(
    signer: Signer,
    envelope: &Envelope,
    answer: &[u8],
    challenge: &Secret<[u8; 32]>,
    caller: &str,
    account_id: &Id,
    device_id: &Id,
) -> Result<Secret<[u8; 32]>, CryptoError> {
    let aad = Aad::signed_browser(signer, BROWSER_HELLO_VERSION, caller, account_id, device_id)?;
    let key = wrapping_key(signer, answer, challenge, signer.browser_info())?;
    key_32(&open(&key, envelope, &aad)?)
}

/// [`seal_signed_unlock`] with a Windows Hello signature (D68).
pub fn seal_windows_hello(
    unlock_key: &Secret<[u8; 32]>,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    account_id: &Id,
    device_id: &Id,
) -> Result<Envelope, CryptoError> {
    seal_signed_unlock(
        Signer::WindowsHello,
        unlock_key,
        signature,
        challenge,
        account_id,
        device_id,
    )
}

/// [`open_signed_unlock`] with a Windows Hello signature (D68).
pub fn open_windows_hello(
    envelope: &Envelope,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    account_id: &Id,
    device_id: &Id,
) -> Result<Secret<[u8; 32]>, CryptoError> {
    open_signed_unlock(
        Signer::WindowsHello,
        envelope,
        signature,
        challenge,
        account_id,
        device_id,
    )
}

/// [`seal_signed_browser`] with a Windows Hello signature (D69).
pub fn seal_browser_hello(
    browser_key: &Secret<[u8; 32]>,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    caller: &str,
    account_id: &Id,
    device_id: &Id,
) -> Result<Envelope, CryptoError> {
    seal_signed_browser(
        Signer::WindowsHello,
        browser_key,
        signature,
        challenge,
        caller,
        account_id,
        device_id,
    )
}

/// [`open_signed_browser`] with a Windows Hello signature (D69).
pub fn open_browser_hello(
    envelope: &Envelope,
    signature: &[u8],
    challenge: &Secret<[u8; 32]>,
    caller: &str,
    account_id: &Id,
    device_id: &Id,
) -> Result<Secret<[u8; 32]>, CryptoError> {
    open_signed_browser(
        Signer::WindowsHello,
        envelope,
        signature,
        challenge,
        caller,
        account_id,
        device_id,
    )
}
