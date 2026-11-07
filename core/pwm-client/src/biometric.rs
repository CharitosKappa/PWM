//! Face ID / Touch ID unlock (decision D45): the opened keyset sealed under a
//! random key. The app keeps the blob and only a hardware-wrapped copy of
//! the key (Secure Enclave, Keystore, Windows Hello), so unlocking needs the
//! biometric check and never the KDF. The browser extension does the same
//! with Touch ID through the Mac app (decision D48 §5), under its own label.

use std::fmt;

use pwm_crypto::{AccountKeyset, Envelope, Secret, Signer, WINDOWS_HELLO_VERSION};
use pwm_store::Store;

use crate::client::parse_id;
use crate::session::open_tokens;
use crate::{Client, ClientError, Tokens};

/// Blob: `v:u8 (= 1) || account_id:16 || envelope`; the AAD binds the device.
const BIOMETRIC_VERSION: u8 = 1;
/// The browser blob: the same layout, its own AAD label (D48 §5).
const BROWSER_UNLOCK_VERSION: u8 = 1;

/// What the extension keeps for Touch ID unlock: the blob on its disk, the
/// key only in the Mac's helper, behind Touch ID.
pub struct BrowserUnlock {
    pub blob: Vec<u8>,
    pub key: Secret<[u8; 32]>,
}

impl fmt::Debug for BrowserUnlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowserUnlock")
            .field("blob_len", &self.blob.len())
            .finish_non_exhaustive()
    }
}

/// What the app keeps for biometric unlock: the blob in storage, the key
/// only wrapped by a hardware key.
pub struct BiometricUnlock {
    pub blob: Vec<u8>,
    pub key: Secret<[u8; 32]>,
}

impl fmt::Debug for BiometricUnlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BiometricUnlock")
            .field("blob_len", &self.blob.len())
            .finish_non_exhaustive()
    }
}

/// What the desktop keeps for Windows Hello unlock (D68): the biometric
/// blob, and the unlock key sealed under a Windows Hello signature's key.
/// The challenge the signature is over is kept beside them; the signature
/// never is.
pub struct WindowsHelloUnlock {
    pub blob: Vec<u8>,
    /// `v:u8 (= 1) || envelope`.
    pub sealed_key: Vec<u8>,
}

impl fmt::Debug for WindowsHelloUnlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowsHelloUnlock")
            .field("blob_len", &self.blob.len())
            .finish_non_exhaustive()
    }
}

/// A new random challenge for Windows Hello to sign (D68 §2).
pub fn windows_hello_challenge() -> Result<Secret<[u8; 32]>, ClientError> {
    pwm_crypto::windows_hello_challenge().map_err(|_| ClientError::Internal)
}

fn challenge_of(challenge: &[u8]) -> Result<Secret<[u8; 32]>, ClientError> {
    let bytes: [u8; 32] = challenge
        .try_into()
        .map_err(|_| ClientError::BiometricUnlockFailed)?;
    Ok(Secret::new(bytes))
}

impl<S: Store> Client<S> {
    /// Windows Hello unlock (D68), and unlock on Linux through the key
    /// holder behind polkit (D71): the biometric blob (D45), with its key
    /// sealed under the key derived from `signature`, the platform key's
    /// answer to `challenge`, with that platform's labels. Needs the vault
    /// unlocked.
    pub fn export_windows_hello(
        &self,
        signer: Signer,
        device_id: &str,
        challenge: &[u8],
        signature: &[u8],
    ) -> Result<WindowsHelloUnlock, ClientError> {
        let device = parse_id(device_id)?;
        let account_id = self.account_record()?.account_id;
        let unlock = self.export_biometric_unlock(device_id)?;
        let sealed = pwm_crypto::seal_signed_unlock(
            signer,
            &unlock.key,
            signature,
            &challenge_of(challenge)?,
            &account_id,
            &device,
        )
        .map_err(|_| ClientError::BiometricUnlockFailed)?;
        let mut sealed_key = vec![WINDOWS_HELLO_VERSION];
        sealed_key.extend_from_slice(&sealed.to_bytes());
        Ok(WindowsHelloUnlock {
            blob: unlock.blob,
            sealed_key,
        })
    }

    /// Unlocks with a new Windows Hello signature over the same challenge,
    /// then answers a fresh blob and sealed key to keep instead (D68 §4).
    /// Anything that doesn't open is `biometric_unlock_failed`.
    #[allow(clippy::too_many_arguments)]
    pub fn unlock_windows_hello(
        &mut self,
        signer: Signer,
        blob: &[u8],
        sealed_key: &[u8],
        challenge: &[u8],
        signature: &[u8],
        device_id: &str,
        tokens: Tokens,
    ) -> Result<WindowsHelloUnlock, ClientError> {
        let device = parse_id(device_id)?;
        let account_id = self
            .store()
            .account()?
            .ok_or(ClientError::BiometricUnlockFailed)?
            .account_id;
        let challenge = challenge_of(challenge)?;
        let key = match sealed_key.split_first() {
            Some((&WINDOWS_HELLO_VERSION, rest)) => Envelope::from_bytes(rest)
                .and_then(|envelope| {
                    pwm_crypto::open_signed_unlock(
                        signer,
                        &envelope,
                        signature,
                        &challenge,
                        &account_id,
                        &device,
                    )
                })
                .map_err(|_| ClientError::BiometricUnlockFailed)?,
            _ => return Err(ClientError::BiometricUnlockFailed),
        };
        self.unlock_biometric(blob, key.expose_secret(), device_id, tokens)?;
        self.export_windows_hello(signer, device_id, challenge.expose_secret(), signature)
    }

    /// The unlocked keyset for biometric unlock on this device. Make it again
    /// at each master password unlock.
    pub fn export_biometric_unlock(&self, device_id: &str) -> Result<BiometricUnlock, ClientError> {
        let device = parse_id(device_id)?;
        let account_id = self.account_record()?.account_id;
        let (envelope, key) = self
            .engine_ref()?
            .keyset()?
            .export_biometric_unlock(&account_id, &device)?;
        let mut blob = vec![BIOMETRIC_VERSION];
        blob.extend_from_slice(&account_id);
        blob.extend_from_slice(&envelope.to_bytes());
        Ok(BiometricUnlock { blob, key })
    }

    /// Unlocks with the blob and the key the hardware unwrapped, without the
    /// master password. Anything that does not open is
    /// `biometric_unlock_failed`: the app then asks for the master password.
    pub fn unlock_biometric(
        &mut self,
        blob: &[u8],
        key: &[u8],
        device_id: &str,
        tokens: Tokens,
    ) -> Result<(), ClientError> {
        let device = parse_id(device_id)?;
        let account_id = self
            .store()
            .account()?
            .ok_or(ClientError::BiometricUnlockFailed)?
            .account_id;
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| ClientError::BiometricUnlockFailed)?;
        let keyset = match blob.split_first() {
            Some((&BIOMETRIC_VERSION, rest)) if rest.len() > 16 && rest[..16] == account_id => {
                Envelope::from_bytes(&rest[16..])
                    .and_then(|envelope| {
                        AccountKeyset::import_biometric_unlock(
                            &envelope,
                            &Secret::new(key),
                            &account_id,
                            &device,
                        )
                    })
                    .map_err(|_| ClientError::BiometricUnlockFailed)?
            }
            _ => return Err(ClientError::BiometricUnlockFailed),
        };
        self.open_unlocked(keyset, tokens)
    }

    /// The extension's unlocked keyset for Touch ID through the Mac app
    /// (D48 §5). Make it again at each master password unlock.
    pub fn export_browser_unlock(&self, device_id: &str) -> Result<BrowserUnlock, ClientError> {
        let device = parse_id(device_id)?;
        let account_id = self.account_record()?.account_id;
        let (envelope, key) = self
            .engine_ref()?
            .keyset()?
            .export_browser_unlock(&account_id, &device)?;
        let mut blob = vec![BROWSER_UNLOCK_VERSION];
        blob.extend_from_slice(&account_id);
        blob.extend_from_slice(&envelope.to_bytes());
        Ok(BrowserUnlock { blob, key })
    }

    /// Unlocks the extension with its blob and the key the Mac's helper
    /// returned after Touch ID, then opens its sealed tokens (D34). Anything
    /// that does not open is `browser_unlock_failed`, and the extension asks
    /// for the master password.
    pub fn unlock_browser(
        &mut self,
        blob: &[u8],
        key: &[u8],
        device_id: &str,
        sealed_tokens: Option<&[u8]>,
    ) -> Result<(), ClientError> {
        let device = parse_id(device_id)?;
        let account_id = self
            .store()
            .account()?
            .ok_or(ClientError::BrowserUnlockFailed)?
            .account_id;
        let key: [u8; 32] = key
            .try_into()
            .map_err(|_| ClientError::BrowserUnlockFailed)?;
        let keyset = match blob.split_first() {
            Some((&BROWSER_UNLOCK_VERSION, rest))
                if rest.len() > 16 && rest[..16] == account_id =>
            {
                Envelope::from_bytes(&rest[16..])
                    .and_then(|envelope| {
                        AccountKeyset::import_browser_unlock(
                            &envelope,
                            &Secret::new(key),
                            &account_id,
                            &device,
                        )
                    })
                    .map_err(|_| ClientError::BrowserUnlockFailed)?
            }
            _ => return Err(ClientError::BrowserUnlockFailed),
        };
        let tokens = open_tokens(&keyset, &account_id, sealed_tokens)?;
        self.open_unlocked(keyset, tokens)
    }
}
