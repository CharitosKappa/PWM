//! JSON export v1 (D24): `{"format": "pwm-export", "version": 1,
//! "exported_at": ms, "vaults": [{"id", "items": [item JSON + "id"]}]}`.
//! Also the `.pwmx` payload.

use pwm_crypto::Secret;
use pwm_vault::{Item, ItemId};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::{Export, ExportError, ExportGrant, ExportItem, ExportVault};

const FORMAT: &str = "pwm-export";
const VERSION: u64 = 1;

/// The complete plaintext export: every item, folder, trashed item and
/// history entry, unchanged.
pub fn export_json(
    grant: ExportGrant,
    now_ms: i64,
    vaults: &[ExportVault],
) -> Result<Secret<Vec<u8>>, ExportError> {
    grant.redeem(now_ms)?;
    write_json(now_ms, vaults, None)
}

/// Serializes the export. With `pad_block`, it is padded (ISO/IEC 7816-4:
/// `0x80`, then zeros) to a multiple of that size. The buffer is reserved
/// up front so no reallocated copy of the plaintext is left behind.
pub(crate) fn write_json(
    now_ms: i64,
    vaults: &[ExportVault],
    pad_block: Option<usize>,
) -> Result<Secret<Vec<u8>>, ExportError> {
    let items = vaults
        .iter()
        .map(|v| {
            v.items
                .iter()
                .map(|i| i.item.to_json_with_id(&i.id))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let head = format!(
        "{{\"format\":\"{FORMAT}\",\"version\":{VERSION},\"exported_at\":{now_ms},\"vaults\":["
    );
    let vault_heads: Vec<String> = vaults
        .iter()
        .map(|v| format!("{{\"id\":\"{}\",\"items\":[", ItemId::from_bytes(v.id)))
        .collect();
    // Separators and closing brackets are counted generously.
    let len = head.len()
        + 2
        + vault_heads
            .iter()
            .zip(&items)
            .map(|(h, its)| {
                h.len()
                    + 3
                    + its
                        .iter()
                        .map(|j| j.expose_secret().len() + 1)
                        .sum::<usize>()
            })
            .sum::<usize>();
    let capacity = match pad_block {
        Some(block) => (len + 1).div_ceil(block) * block,
        None => len,
    };
    Ok(Secret::<Vec<u8>>::new_with(|buf| {
        buf.reserve_exact(capacity);
        buf.extend_from_slice(head.as_bytes());
        for (n, (vault_head, vault_items)) in vault_heads.iter().zip(&items).enumerate() {
            if n > 0 {
                buf.push(b',');
            }
            buf.extend_from_slice(vault_head.as_bytes());
            for (m, item) in vault_items.iter().enumerate() {
                if m > 0 {
                    buf.push(b',');
                }
                buf.extend_from_slice(item.expose_secret().as_bytes());
            }
            buf.extend_from_slice(b"]}");
        }
        buf.extend_from_slice(b"]}");
        if let Some(block) = pad_block {
            buf.push(0x80);
            buf.resize(buf.len().div_ceil(block) * block, 0);
        }
    }))
}

#[derive(Deserialize)]
struct Probe {
    format: Option<String>,
    version: Option<u64>,
}

#[derive(Deserialize)]
struct RawExport<'a> {
    exported_at: i64,
    #[serde(borrow)]
    vaults: Vec<RawVault<'a>>,
}

#[derive(Deserialize)]
struct RawVault<'a> {
    id: ItemId,
    #[serde(borrow)]
    items: Vec<&'a RawValue>,
}

/// Parses a JSON export (or a decrypted `.pwmx` payload).
pub fn read_json(bytes: &[u8]) -> Result<Export, ExportError> {
    let probe: Probe = serde_json::from_slice(bytes).map_err(|_| ExportError::NotAnExport)?;
    if probe.format.as_deref() != Some(FORMAT) {
        return Err(ExportError::NotAnExport);
    }
    match probe.version {
        Some(VERSION) => {}
        Some(other) => return Err(ExportError::UnsupportedVersion(other)),
        None => return Err(ExportError::NotAnExport),
    }
    let raw: RawExport = serde_json::from_slice(bytes).map_err(|_| ExportError::NotAnExport)?;
    let vaults = raw
        .vaults
        .iter()
        .map(|v| {
            let items = v
                .items
                .iter()
                .map(|json| {
                    let (id, item) = Item::from_json_with_id(json.get())?;
                    Ok(ExportItem { id, item })
                })
                .collect::<Result<Vec<_>, ExportError>>()?;
            Ok(ExportVault {
                id: *v.id.as_bytes(),
                items,
            })
        })
        .collect::<Result<Vec<_>, ExportError>>()?;
    Ok(Export {
        exported_at: raw.exported_at,
        vaults,
    })
}
