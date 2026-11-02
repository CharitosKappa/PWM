//! Associated data that binds every envelope to its context (decision D11,
//! `docs/SECURITY.md` "Encodings").

use crate::{CryptoError, KdfParams};

/// A 16-byte random identifier (UUIDv4) for accounts, vaults and items.
pub type Id = [u8; 16];

/// A new random [`Id`] with the UUIDv4 version and variant bits set.
pub fn generate_id() -> Result<Id, CryptoError> {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(|_| CryptoError::Rng)?;
    id[6] = (id[6] & 0x0F) | 0x40;
    id[8] = (id[8] & 0x3F) | 0x80;
    Ok(id)
}

/// AAD for one envelope: `len(label):u8 || label || fields`, integers
/// big-endian. Only the constructors below can build one, so every envelope
/// carries a purpose label and ciphertexts cannot be moved between contexts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aad(Vec<u8>);

impl Aad {
    /// Item blob encrypted under a vault key.
    pub fn item(version: u8, vault_id: &Id, item_id: &Id, revision: u64) -> Self {
        let mut aad = Self::labelled("pwm/item");
        aad.0.push(version);
        aad.0.extend_from_slice(vault_id);
        aad.0.extend_from_slice(item_id);
        aad.0.extend_from_slice(&revision.to_be_bytes());
        aad
    }

    /// Vault key encrypted under the account `sym_key`.
    pub fn vault_key(version: u8, vault_id: &Id) -> Self {
        let mut aad = Self::labelled("pwm/vault-key");
        aad.0.push(version);
        aad.0.extend_from_slice(vault_id);
        aad
    }

    /// Account keyset encrypted under the AUK.
    pub fn keyset(version: u8, account_id: &Id) -> Self {
        let mut aad = Self::labelled("pwm/keyset");
        aad.0.push(version);
        aad.0.extend_from_slice(account_id);
        aad
    }

    /// Vault key shared to a recipient (decision D17).
    pub fn share(
        version: u8,
        vault_id: &Id,
        sender_ed25519_pub: &[u8; 32],
        recipient_x25519_pub: &[u8; 32],
    ) -> Self {
        let mut aad = Self::labelled("pwm/share");
        aad.0.push(version);
        aad.0.extend_from_slice(vault_id);
        aad.0.extend_from_slice(sender_ed25519_pub);
        aad.0.extend_from_slice(recipient_x25519_pub);
        aad
    }

    /// `.pwmx` export file header (decision D24).
    pub fn export(version: u8, params: &KdfParams, salt: &[u8; 16]) -> Self {
        let mut aad = Self::labelled("pwm/export");
        aad.0.push(version);
        aad.0.extend_from_slice(&params.memory_kib.to_be_bytes());
        aad.0.extend_from_slice(&params.iterations.to_be_bytes());
        aad.0.extend_from_slice(&params.parallelism.to_be_bytes());
        aad.0.extend_from_slice(salt);
        aad
    }

    /// Device name sealed under the account `sym_key` (decision D26).
    pub fn device_name(version: u8, account_id: &Id, device_id: &Id) -> Self {
        let mut aad = Self::labelled("pwm/device-name");
        aad.0.push(version);
        aad.0.extend_from_slice(account_id);
        aad.0.extend_from_slice(device_id);
        aad
    }

    /// OPAQUE login state the server keeps between login start and finish
    /// (decision D26).
    pub fn login_state(version: u8, account_id: &Id) -> Self {
        let mut aad = Self::labelled("pwm/login-state");
        aad.0.push(version);
        aad.0.extend_from_slice(account_id);
        aad
    }

    /// Unlocked keyset in the extension's memory-only session store,
    /// under a random session key (decision D34).
    pub fn session(version: u8, account_id: &Id) -> Self {
        let mut aad = Self::labelled("pwm/session");
        aad.0.push(version);
        aad.0.extend_from_slice(account_id);
        aad
    }

    /// A secret the device keeps on disk (the refresh token), under the
    /// account `sym_key` (decision D34).
    pub fn device_tokens(version: u8, account_id: &Id) -> Self {
        let mut aad = Self::labelled("pwm/device-tokens");
        aad.0.push(version);
        aad.0.extend_from_slice(account_id);
        aad
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn labelled(label: &'static str) -> Self {
        let len = u8::try_from(label.len()).expect("AAD labels are short ASCII constants");
        let mut bytes = Vec::with_capacity(1 + label.len() + 41);
        bytes.push(len);
        bytes.extend_from_slice(label.as_bytes());
        Self(bytes)
    }
}
