//! Our own exports (D24): `.pwmx`, JSON and CSV, through `pwm-export`'s
//! readers. Every item gets a new id, and folder references follow, so an
//! export goes into any account without clashing with what is there.

use std::collections::HashMap;

use pwm_crypto::Secret;
use pwm_export::{CSV_COLUMNS, ExportError, ExportItem};
use pwm_vault::{ItemId, ItemKind};

use crate::build::Builder;
use crate::{ImportError, Parsed};

pub fn is_pwmx(bytes: &[u8]) -> bool {
    bytes.starts_with(&pwm_crypto::EXPORT_MAGIC)
}

pub fn is_own_csv(headers: &[String]) -> bool {
    headers.len() == CSV_COLUMNS.len() && headers.iter().zip(CSV_COLUMNS).all(|(h, c)| h == c)
}

pub fn parse(
    bytes: &[u8],
    password: Option<&Secret<String>>,
    now_ms: i64,
) -> Result<Parsed, ImportError> {
    let items: Vec<ExportItem> = if is_pwmx(bytes) {
        let password = password.ok_or(ImportError::PasswordNeeded)?;
        let export =
            pwm_export::read_pwmx(bytes, password.expose_secret()).map_err(|e| match e {
                // Only the key's check fails on a wrong password.
                ExportError::Crypto(_) => ImportError::WrongPassword,
                _ => ImportError::Unreadable,
            })?;
        export.vaults.into_iter().flat_map(|v| v.items).collect()
    } else if crate::csv::strip_bom(bytes)
        .iter()
        .copied()
        .find(|c| !c.is_ascii_whitespace())
        == Some(b'{')
    {
        let export = pwm_export::read_json(crate::csv::strip_bom(bytes)).map_err(map)?;
        export.vaults.into_iter().flat_map(|v| v.items).collect()
    } else {
        pwm_export::read_csv(crate::csv::strip_bom(bytes), now_ms).map_err(map)?
    };
    if items.len() > crate::MAX_ITEMS {
        return Err(ImportError::TooLarge);
    }
    // New ids, and folder references mapped to them.
    let new_ids: HashMap<ItemId, ItemId> = items
        .iter()
        .map(|i| Ok((i.id, ItemId::generate()?)))
        .collect::<Result<_, ImportError>>()?;
    let remap = |id: Option<ItemId>| id.and_then(|id| new_ids.get(&id).copied());
    let mut b = Builder::new(now_ms);
    let (folders, others): (Vec<ExportItem>, Vec<ExportItem>) = items
        .into_iter()
        .partition(|i| matches!(i.item.kind, ItemKind::Folder(_)));
    let mut out = Vec::with_capacity(folders.len() + others.len());
    for ExportItem { id, mut item } in folders.into_iter().chain(others) {
        item.folder_id = remap(item.folder_id);
        if let ItemKind::Folder(folder) = &mut item.kind {
            folder.parent_id = remap(folder.parent_id);
        }
        b.report_item(&item);
        out.push((new_ids[&id], item));
    }
    Ok(Parsed {
        items: out,
        report: b.finish().report,
    })
}

fn map(_: ExportError) -> ImportError {
    ImportError::Unreadable
}
