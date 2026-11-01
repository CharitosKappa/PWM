//! Item record v1: padded JSON sealed under the vault key (decision D18,
//! `docs/SECURITY.md` "Item record v1").

use pwm_crypto::{Aad, Envelope, Id, Secret, SymmetricKey, open, seal};

use crate::{Item, ItemId, VaultError};

/// Record format version bound into the item AAD: the padding and JSON
/// container. The JSON schema version (`v`) lives inside the record.
const RECORD_VERSION: u8 = 1;
const PAD_BLOCK: usize = 256;

/// Encrypts an item for storage under `revision`. Read-only items (newer
/// schema or unknown type) and invalid items are refused.
pub fn encrypt_item(
    vault_key: &SymmetricKey,
    vault_id: &Id,
    item_id: &ItemId,
    revision: u64,
    item: &Item,
) -> Result<Envelope, VaultError> {
    if !item.is_editable() {
        return Err(VaultError::ReadOnly);
    }
    item.validate()?;
    let mut result = Ok(());
    let plaintext = Secret::<Vec<u8>>::new_with(|buf| {
        // Reserve up front so serialization does not leave reallocated copies.
        buf.reserve(4 * PAD_BLOCK);
        result = item.write_json(buf);
        pad(buf);
    });
    result?;
    let aad = Aad::item(RECORD_VERSION, vault_id, item_id.as_bytes(), revision);
    Ok(seal(vault_key, plaintext.expose_secret(), &aad)?)
}

/// Decrypts and parses an item stored under `revision`.
pub fn decrypt_item(
    vault_key: &SymmetricKey,
    vault_id: &Id,
    item_id: &ItemId,
    revision: u64,
    envelope: &Envelope,
) -> Result<Item, VaultError> {
    let aad = Aad::item(RECORD_VERSION, vault_id, item_id.as_bytes(), revision);
    let plaintext = open(vault_key, envelope, &aad)?;
    Item::from_slice(unpad(plaintext.expose_secret())?)
}

/// ISO/IEC 7816-4 padding to the next multiple of [`PAD_BLOCK`]: one `0x80`
/// byte, then zeros.
fn pad(buf: &mut Vec<u8>) {
    buf.push(0x80);
    let padded = buf.len().div_ceil(PAD_BLOCK) * PAD_BLOCK;
    buf.resize(padded, 0);
}

fn unpad(buf: &[u8]) -> Result<&[u8], VaultError> {
    if buf.is_empty() || !buf.len().is_multiple_of(PAD_BLOCK) {
        return Err(VaultError::InvalidItem);
    }
    let end = buf
        .iter()
        .rposition(|&b| b != 0)
        .ok_or(VaultError::InvalidItem)?;
    if buf[end] != 0x80 {
        return Err(VaultError::InvalidItem);
    }
    Ok(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_and_unpad() {
        for len in [0usize, 1, 254, 255, 256, 257, 511, 512] {
            let data = vec![b'x'; len];
            let mut buf = data.clone();
            pad(&mut buf);
            assert_eq!(buf.len() % PAD_BLOCK, 0, "len {len}");
            assert!(buf.len() > len, "len {len}");
            assert_eq!(unpad(&buf).unwrap(), data.as_slice(), "len {len}");
        }
    }

    #[test]
    fn unpad_rejects_bad_padding() {
        let mut no_marker = vec![b'x'; 10];
        no_marker.resize(PAD_BLOCK, 0);
        let mut wrong_marker = vec![b'x'; 10];
        wrong_marker.push(0x81);
        wrong_marker.resize(PAD_BLOCK, 0);
        for bad in [
            vec![],
            vec![0u8; PAD_BLOCK],
            no_marker,
            wrong_marker,
            vec![0x80; 100],
        ] {
            assert!(unpad(&bad).is_err());
        }
    }
}
