//! What happens to one cached item (D28 §3, §5–§7, §9). Revision rules: a
//! `Modified` record at revision R is based on server revision R - 1
//! (R = 1: not on the server yet); a `Deleted` record holds the server
//! revision it deletes (1 for an item never confirmed, D50 §4).
//!
//! The vault key is `None` in a locked sync (D50 §1). A step that needs it
//! returns [`SyncError::Locked`] before writing anything, so the caller can
//! leave the item for the next unlocked sync.

use pwm_crypto::{Envelope, Id, SymmetricKey};
use pwm_store::{ItemRecord, Store, SyncState};
use pwm_vault::{Item, ItemId, decrypt_item, encrypt_item, encrypt_item_fitting};

use crate::SyncError;

/// The key, or `Locked` when syncing without keys.
pub(crate) fn need(key: Option<&SymmetricKey>) -> Result<&SymmetricKey, SyncError> {
    key.ok_or(SyncError::Locked)
}

pub(crate) fn put<S: Store>(
    store: &mut S,
    vault_id: &Id,
    item_id: &ItemId,
    revision: u64,
    envelope: Envelope,
    state: SyncState,
) -> Result<(), SyncError> {
    store.put_items(&[ItemRecord {
        vault_id: *vault_id,
        item_id: *item_id,
        revision,
        envelope,
        state,
    }])?;
    Ok(())
}

fn decrypt(key: &SymmetricKey, record: &ItemRecord) -> Result<Item, SyncError> {
    Ok(decrypt_item(
        key,
        &record.vault_id,
        &record.item_id,
        record.revision,
        &record.envelope,
    )?)
}

/// Seals the record's content again at `revision`, as `Modified`.
pub(crate) fn reseal<S: Store>(
    store: &mut S,
    key: Option<&SymmetricKey>,
    record: &ItemRecord,
    revision: u64,
) -> Result<(), SyncError> {
    let envelope = if revision == record.revision {
        record.envelope.clone()
    } else {
        let key = need(key)?;
        let item = decrypt(key, record)?;
        encrypt_item(key, &record.vault_id, &record.item_id, revision, &item)?
    };
    put(
        store,
        &record.vault_id,
        &record.item_id,
        revision,
        envelope,
        SyncState::Modified,
    )
}

/// Saves `item` as a new item (new id, revision 1, `Modified`); returns
/// its id. Used when two versions cannot live under one id.
pub(crate) fn save_copy<S: Store>(
    store: &mut S,
    key: &SymmetricKey,
    vault_id: &Id,
    item: &Item,
) -> Result<ItemId, SyncError> {
    let id = ItemId::generate()?;
    let envelope = encrypt_item_fitting(key, vault_id, &id, 1, &mut item.clone())?;
    put(store, vault_id, &id, 1, envelope, SyncState::Modified)?;
    Ok(id)
}

/// Moves an unsent edit to a new id because its id was deleted on the
/// server (ids are never reused, D27). The copy is written before the old
/// record goes, so a crash in between leaves both, never neither.
pub(crate) fn move_to_copy<S: Store>(
    store: &mut S,
    key: Option<&SymmetricKey>,
    local: &ItemRecord,
) -> Result<Option<ItemId>, SyncError> {
    let key = need(key)?;
    let copy = match decrypt(key, local) {
        Ok(item) if item.is_editable() => Some(save_copy(store, key, &local.vault_id, &item)?),
        _ => None,
    };
    store.delete_item(&local.vault_id, &local.item_id)?;
    Ok(copy)
}

/// Both sides changed the item: the later `updated_at` stays current (a
/// tie keeps the server's), the other goes into history, and the result is
/// sealed at `server_revision + 1` for upload. Versions that cannot be
/// merged (read-only or unreadable) are kept side by side: the server's
/// stays, the local one becomes a new item. Returns that new item's id.
pub(crate) fn merge<S: Store>(
    store: &mut S,
    key: Option<&SymmetricKey>,
    local: &ItemRecord,
    server_revision: u64,
    server_envelope: Envelope,
    now_ms: i64,
) -> Result<Option<ItemId>, SyncError> {
    let key = need(key)?;
    let (vault_id, item_id) = (local.vault_id, local.item_id);
    let ours = decrypt(key, local).ok().filter(Item::is_editable);
    let theirs = decrypt_item(key, &vault_id, &item_id, server_revision, &server_envelope).ok();
    match (ours, theirs) {
        (Some(ours), Some(theirs)) if theirs.is_editable() => {
            let (mut winner, loser) = if ours.updated_at > theirs.updated_at {
                (ours, theirs)
            } else {
                (theirs, ours)
            };
            winner.keep_conflicting(loser, now_ms)?;
            let revision = server_revision + 1;
            // The loser's versions can push it over the limit (D29).
            let envelope = encrypt_item_fitting(key, &vault_id, &item_id, revision, &mut winner)?;
            put(
                store,
                &vault_id,
                &item_id,
                revision,
                envelope,
                SyncState::Modified,
            )?;
            Ok(None)
        }
        (ours, _) => {
            put(
                store,
                &vault_id,
                &item_id,
                server_revision,
                server_envelope,
                SyncState::Synced,
            )?;
            ours.map(|item| save_copy(store, key, &vault_id, &item))
                .transpose()
        }
    }
}

/// How applying one pulled change went.
#[derive(Default)]
pub(crate) struct Applied {
    pub conflict: bool,
}

/// Applies one change from delta sync to the cache.
pub(crate) fn apply_change<S: Store>(
    store: &mut S,
    key: Option<&SymmetricKey>,
    vault_id: &Id,
    item_id: &ItemId,
    revision: u64,
    envelope: Option<Envelope>,
    now_ms: i64,
) -> Result<Applied, SyncError> {
    let local = store.item(vault_id, item_id)?;
    let mut applied = Applied::default();
    let Some(envelope) = envelope else {
        // A tombstone.
        match local {
            Some(l) if l.state == SyncState::Modified => {
                move_to_copy(store, key, &l)?;
                applied.conflict = true;
            }
            Some(_) => store.delete_item(vault_id, item_id)?,
            None => {}
        }
        return Ok(applied);
    };
    let take = |store: &mut S, envelope| {
        put(
            store,
            vault_id,
            item_id,
            revision,
            envelope,
            SyncState::Synced,
        )
    };
    match local {
        None => take(store, envelope)?,
        Some(l) => match l.state {
            // The server has less than we had synced (restored from a
            // backup): our version goes up again on top of it.
            SyncState::Synced if revision < l.revision => {
                match reseal(store, key, &l, revision + 1) {
                    Err(SyncError::Locked) => return Err(SyncError::Locked),
                    // Unreadable: the server's version stays.
                    Err(_) => take(store, envelope)?,
                    Ok(()) => {}
                }
            }
            SyncState::Synced => take(store, envelope)?,
            // Our own upload coming back (its answer was lost).
            SyncState::Modified if l.envelope == envelope => take(store, envelope)?,
            // Not newer than our base: the upload will sort it out.
            SyncState::Modified if revision < l.revision => {}
            SyncState::Modified => {
                merge(store, key, &l, revision, envelope, now_ms)?;
                applied.conflict = true;
            }
            // Edited after the version we deleted: the edit wins.
            SyncState::Deleted if revision > l.revision => take(store, envelope)?,
            SyncState::Deleted => {}
        },
    }
    Ok(applied)
}

/// After a full resync: items we hold as synced that the server no longer
/// has go up again as new (revision 1).
pub(crate) fn reupload_missing<S: Store>(
    store: &mut S,
    key: &SymmetricKey,
    vault_id: &Id,
    seen: &std::collections::HashSet<ItemId>,
) -> Result<(), SyncError> {
    for record in store.items(vault_id)? {
        if record.state == SyncState::Synced && !seen.contains(&record.item_id) {
            // An unreadable record cannot be resealed; it stays as it is.
            if let Err(SyncError::Store(err)) = reseal(store, Some(key), &record, 1) {
                return Err(err.into());
            }
        }
    }
    Ok(())
}

/// After a full pull caused by compacted tombstones (D76 §2): the server
/// wasn't restored, so an item it no longer has was deleted there.
/// - **Never uploaded** (`Modified` at revision 1): it goes up as usual.
/// - **An unsent edit:** kept as a new item, as for a tombstone (D28).
/// - **Synced, or deleted here too:** removed.
pub(crate) fn drop_missing<S: Store>(
    store: &mut S,
    key: &SymmetricKey,
    vault_id: &Id,
    seen: &std::collections::HashSet<ItemId>,
) -> Result<(), SyncError> {
    for record in store.items(vault_id)? {
        if seen.contains(&record.item_id) {
            continue;
        }
        match record.state {
            SyncState::Modified if record.revision == 1 => {}
            SyncState::Modified => {
                move_to_copy(store, Some(key), &record)?;
            }
            SyncState::Synced | SyncState::Deleted => {
                store.delete_item(vault_id, &record.item_id)?;
            }
        }
    }
    Ok(())
}
