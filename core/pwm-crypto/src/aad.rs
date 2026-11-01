//! Associated data that binds every envelope to its context (decision D11,
//! `docs/SECURITY.md` "Encodings").

/// A 16-byte random identifier (UUIDv4) for accounts, vaults and items.
pub type Id = [u8; 16];

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
