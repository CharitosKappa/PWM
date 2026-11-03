//! Face ID / Touch ID unlock (decision D45): the opened keyset sealed under a
//! random key. The app keeps the blob and only a hardware-wrapped copy of
//! the key (Secure Enclave, Keystore, Windows Hello), so unlocking needs the
//! biometric check and never the KDF.

use std::fmt;

use pwm_crypto::{AccountKeyset, Envelope, Secret};
use pwm_store::Store;

use crate::client::parse_id;
use crate::{Client, ClientError, Tokens};

/// Blob: `v:u8 (= 1) || account_id:16 || envelope`; the AAD binds the device.
const BIOMETRIC_VERSION: u8 = 1;

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
}
