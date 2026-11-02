//! Item history and trash (decision D19). Both live inside the encrypted
//! item, so the server never learns how an item changed or what is trashed.

use std::fmt;

use crate::{Item, ItemId, ItemKind, VaultError, remove_folder_refs};

/// Previous versions kept per item.
pub const HISTORY_LIMIT: usize = 10;

/// Trashed items are purged by the client after this long (30 days).
pub const TRASH_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// A previous version of an item. `item` has no history of its own.
#[derive(Clone, PartialEq)]
pub struct ItemVersion {
    /// When this version was replaced (Unix ms).
    pub saved_at: i64,
    pub item: Item,
}

impl fmt::Debug for ItemVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ItemVersion([REDACTED])")
    }
}

impl Item {
    /// Previous versions, newest first.
    pub fn history(&self) -> &[ItemVersion] {
        &self.history
    }

    /// Replaces this item's content with `edited` (as changed in the UI).
    /// The previous version goes into history unless only `favorite`
    /// changed. Creation time, trash state and history are kept from `self`.
    pub fn apply_edit(&mut self, mut edited: Item, now_ms: i64) -> Result<(), VaultError> {
        self.ensure_editable()?;
        if !edited.is_editable() {
            return Err(VaultError::ReadOnly);
        }
        if !self.same_content(&edited) {
            let previous = self.snapshot();
            self.push_version(previous, now_ms);
            self.take_content(&mut edited);
        }
        self.favorite = edited.favorite;
        self.updated_at = now_ms;
        Ok(())
    }

    /// Keeps `version` in history without changing the current content, for
    /// the losing side of a sync conflict (M3.4).
    pub fn keep_version(&mut self, version: Item, saved_at: i64) -> Result<(), VaultError> {
        self.ensure_editable()?;
        self.push_version(version, saved_at);
        Ok(())
    }

    /// Keeps the losing side of a sync conflict (D28): `other` as a version
    /// saved at `saved_at`, plus every version in its history that this
    /// item lacks, so edits made offline on the losing device survive too.
    /// History stays newest first and at most [`HISTORY_LIMIT`] long.
    pub fn keep_conflicting(&mut self, mut other: Item, saved_at: i64) -> Result<(), VaultError> {
        self.ensure_editable()?;
        for version in std::mem::take(&mut other.history) {
            if !self.history.iter().any(|v| v.item == version.item) {
                self.history.push(version);
            }
        }
        self.history.push(ItemVersion {
            saved_at,
            item: other,
        });
        // Stable, so versions saved at the same time keep their order.
        self.history.sort_by_key(|v| std::cmp::Reverse(v.saved_at));
        self.history.truncate(HISTORY_LIMIT);
        Ok(())
    }

    /// Makes history entry `index` the current content again. The current
    /// content goes into history first, so nothing is lost.
    pub fn restore_version(&mut self, index: usize, now_ms: i64) -> Result<(), VaultError> {
        self.ensure_editable()?;
        let mut restored = self
            .history
            .get(index)
            .ok_or(VaultError::Invalid("no such version"))?
            .item
            .clone();
        let current = self.snapshot();
        self.push_version(current, now_ms);
        self.take_content(&mut restored);
        self.updated_at = now_ms;
        Ok(())
    }

    pub fn is_trashed(&self) -> bool {
        self.trashed_at.is_some()
    }

    /// Moves the item to the trash. Hidden from lists, search and autofill
    /// until restored or purged.
    pub fn trash(&mut self, now_ms: i64) -> Result<(), VaultError> {
        self.ensure_editable()?;
        self.trashed_at = Some(now_ms);
        self.updated_at = now_ms;
        Ok(())
    }

    pub fn restore_from_trash(&mut self, now_ms: i64) -> Result<(), VaultError> {
        self.ensure_editable()?;
        self.trashed_at = None;
        self.updated_at = now_ms;
        Ok(())
    }

    /// True once the item has been in the trash for [`TRASH_RETENTION_MS`];
    /// the client then deletes it for good (server tombstone).
    pub fn is_due_for_purge(&self, now_ms: i64) -> bool {
        self.trashed_at
            .is_some_and(|t| now_ms.saturating_sub(t) >= TRASH_RETENTION_MS)
    }

    fn ensure_editable(&self) -> Result<(), VaultError> {
        if self.is_editable() {
            Ok(())
        } else {
            Err(VaultError::ReadOnly)
        }
    }

    /// Equal apart from favorite, timestamps, trash state and history.
    fn same_content(&self, other: &Item) -> bool {
        self.kind == other.kind
            && self.title == other.title
            && self.notes == other.notes
            && self.folder_id == other.folder_id
            && self.tags == other.tags
            && self.custom_fields == other.custom_fields
            && self.extra == other.extra
    }

    /// Moves the content fields of `from` into `self`.
    fn take_content(&mut self, from: &mut Item) {
        self.kind = std::mem::replace(&mut from.kind, ItemKind::Note);
        self.title = std::mem::take(&mut from.title);
        self.notes = std::mem::take(&mut from.notes);
        self.folder_id = from.folder_id;
        self.tags = std::mem::take(&mut from.tags);
        self.custom_fields = std::mem::take(&mut from.custom_fields);
        self.extra = std::mem::take(&mut from.extra);
    }

    /// This item without its history.
    fn snapshot(&self) -> Item {
        Item {
            kind: self.kind.clone(),
            title: self.title.clone(),
            notes: self.notes.clone(),
            favorite: self.favorite,
            folder_id: self.folder_id,
            tags: self.tags.clone(),
            custom_fields: self.custom_fields.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            trashed_at: self.trashed_at,
            history: Vec::new(),
            schema_version: self.schema_version,
            extra: self.extra.clone(),
        }
    }

    fn push_version(&mut self, mut version: Item, saved_at: i64) {
        version.history.clear();
        self.history.insert(
            0,
            ItemVersion {
                saved_at,
                item: version,
            },
        );
        self.history.truncate(HISTORY_LIMIT);
    }
}

/// Trashes `folder` and moves its items to no folder right away; restoring
/// the folder later does not move them back. Returns how many items moved.
pub fn trash_folder(
    folder: &mut Item,
    folder_id: &ItemId,
    items: &mut [Item],
    now_ms: i64,
) -> Result<usize, VaultError> {
    if !matches!(folder.kind, ItemKind::Folder(_)) {
        return Err(VaultError::Invalid("not a folder"));
    }
    folder.trash(now_ms)?;
    Ok(remove_folder_refs(items, folder_id))
}
