//! Face ID / Touch ID unlock (decision D45): the opened keyset sealed under a
//! random key. The app keeps the blob and only a hardware-wrapped copy of
//! the key (Secure Enclave, Keystore, Windows Hello), so unlocking needs the
//! biometric check and never the KDF. The browser extension does the same
//! with Touch ID through the Mac app (decision D48 §5), under its own label.

use std::fmt;

use pwm_crypto::{AccountKeyset, Envelope, Secret};
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

impl<S: Store> Client<S> {
    /// The unlocked keyset for biometric unlock on this device. Make it again
    /// at each master password unlock.
    pub fn export_biometric_unlock(&self, device_id: &str) -> Result<BiometricUnlock, ClientError> {
        let device = parse_id(device_id)?;
        let account_id = self.account_record()?.account_id;
        let (envelope, key) = self
            .engine_ref()?
            .keyset()
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
            .keyset()
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
