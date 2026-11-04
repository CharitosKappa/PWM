//! Importing (D51): read a picked file in memory, show what will come in,
//! then save it into a vault with new ids. Items identical to ones already
//! in that vault are skipped (§8), folders with the same name and parent are
//! reused, and items over the sync size limit are left out and reported.
//! No master password is asked: importing only adds items (§9).

use std::collections::HashSet;

use pwm_crypto::{Id, Secret};
use pwm_import::{ColumnMapping, Format, LeftOut, Options, Parsed, Report};
use pwm_store::Store;
use pwm_vault::{Item, ItemId, ItemKind};

use crate::client::parse_id;
use crate::{Client, ClientError};

/// What an import brings in and leaves out, for the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPreview {
    pub logins: usize,
    pub notes: usize,
    pub cards: usize,
    pub identities: usize,
    pub folders: usize,
    /// Of the above, how many come in trashed.
    pub trashed: usize,
    /// `(reason code, count)`, such as `("attachments", 2)` or
    /// `("duplicates", 5)`.
    pub left_out: Vec<(String, usize)>,
}

impl From<&Report> for ImportPreview {
    fn from(r: &Report) -> Self {
        Self {
            logins: r.logins,
            notes: r.notes,
            cards: r.cards,
            identities: r.identities,
            folders: r.folders,
            trashed: r.trashed,
            left_out: r
                .left_out
                .iter()
                .map(|(reason, n)| (reason.as_str().to_owned(), *n))
                .collect(),
        }
    }
}

/// An import read and checked, waiting for "Import".
pub(crate) struct PendingImport {
    vault_id: Id,
    parsed: Parsed,
}

/// A format and whether it needs a password, for the screen to suggest.
pub fn detect_import(file_name: &str, bytes: &[u8]) -> Option<(String, bool)> {
    pwm_import::detect(file_name, bytes).map(|d| (d.format.as_str().to_owned(), d.needs_password))
}

impl From<pwm_import::ImportError> for ClientError {
    fn from(err: pwm_import::ImportError) -> Self {
        match err {
            pwm_import::ImportError::Crypto(e) => e.into(),
            other => Self::Import(other.code()),
        }
    }
}

/// What makes two items the same for the duplicate check: the type, title,
/// username, password and URLs (D51 §8); for other types their main values.
fn same_key(item: &Item) -> Option<String> {
    let mut key = String::new();
    let mut push = |s: &str| {
        key.push_str(s);
        key.push('\u{0}');
    };
    push(&item.title);
    match &item.kind {
        ItemKind::Login(login) => {
            push("login");
            push(&login.username);
            push(&login.password);
            let mut urls: Vec<&str> = login.urls.iter().map(|u| u.url.as_str()).collect();
            urls.sort_unstable();
            urls.iter().for_each(|u| push(u));
        }
        ItemKind::Card(card) => {
            push("card");
            push(&card.number);
        }
        ItemKind::Identity(identity) => {
            push("identity");
            push(&identity.first_name);
            push(&identity.last_name);
            push(&identity.email);
        }
        ItemKind::Note => {
            push("note");
            push(&item.notes);
        }
        ItemKind::Folder(_) | ItemKind::Unknown(_) => return None,
    }
    Some(key)
}

impl<S: Store> Client<S> {
    /// Reads `bytes` as `format` (from [`detect_import`] or the user's
    /// choice) and checks it against `vault_id` (default: the first vault).
    /// `password` is the export's own; `key_file` is a KeePass key file.
    /// Nothing is saved until [`finish_import`](Self::finish_import).
    // Each input is one thing the import screen collects.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_import(
        &mut self,
        vault_id: Option<&str>,
        format: &str,
        bytes: &[u8],
        password: Option<&str>,
        key_file: Option<&[u8]>,
        mapping: Option<ColumnMapping>,
        now_ms: i64,
    ) -> Result<ImportPreview, ClientError> {
        self.import = None;
        let vault_id = match vault_id {
            Some(id) => parse_id(id)?,
            None => parse_id(self.vault_ids()?.first().ok_or(ClientError::UnknownVault)?)?,
        };
        let format = Format::parse(format).ok_or(ClientError::Import("import_unreadable"))?;
        let options = Options {
            password: password.map(|p| Secret::new(p.to_owned())),
            key_file: key_file.map(|k| Secret::new(k.to_vec())),
            mapping,
            now_ms,
        };
        let mut parsed = pwm_import::parse(format, bytes, &options)?;
        self.dedupe(&vault_id, &mut parsed)?;
        let preview = ImportPreview::from(&parsed.report);
        self.import = Some(PendingImport { vault_id, parsed });
        Ok(preview)
    }

    /// Skips items already in the vault and reuses its folders of the same
    /// name and parent.
    fn dedupe(&self, vault_id: &Id, parsed: &mut Parsed) -> Result<(), ClientError> {
        let existing = self.engine_ref()?.items(vault_id)?;
        let keys: HashSet<String> = existing
            .iter()
            .filter(|(_, i)| i.trashed_at.is_none())
            .filter_map(|(_, i)| same_key(i))
            .collect();
        // Existing folders by (name, parent).
        let folder_of = |name: &str, parent: Option<ItemId>| {
            existing.iter().find_map(|(id, i)| match &i.kind {
                ItemKind::Folder(f)
                    if i.title == name && f.parent_id == parent && i.trashed_at.is_none() =>
                {
                    Some(*id)
                }
                _ => None,
            })
        };
        let mut reused: Vec<(ItemId, ItemId)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut kept = Vec::with_capacity(parsed.items.len());
        let mut duplicates = 0;
        for (id, mut item) in std::mem::take(&mut parsed.items) {
            let map = |id: Option<ItemId>, reused: &[(ItemId, ItemId)]| {
                id.map(|id| {
                    reused
                        .iter()
                        .find(|(from, _)| *from == id)
                        .map_or(id, |(_, to)| *to)
                })
            };
            item.folder_id = map(item.folder_id, &reused);
            if let ItemKind::Folder(folder) = &mut item.kind {
                folder.parent_id = map(folder.parent_id, &reused);
                if let Some(existing) = folder_of(&item.title, folder.parent_id) {
                    reused.push((id, existing));
                    parsed.report.uncount(&item);
                    continue;
                }
            } else if let Some(key) = same_key(&item)
                && item.trashed_at.is_none()
                && (keys.contains(&key) || !seen.insert(key))
            {
                duplicates += 1;
                parsed.report.uncount(&item);
                continue;
            }
            kept.push((id, item));
        }
        parsed.items = kept;
        parsed.report.leave_out(LeftOut::Duplicates, duplicates);
        Ok(())
    }

    /// Saves the prepared import into its vault; returns what came in.
    /// Sync afterwards to upload it.
    pub fn finish_import(&mut self, _now_ms: i64) -> Result<ImportPreview, ClientError> {
        let PendingImport {
            vault_id,
            mut parsed,
        } = self.import.take().ok_or(ClientError::NotRunning)?;
        let engine = self.engine()?;
        let mut too_large = 0;
        let mut dropped: Vec<ItemId> = Vec::new();
        for (id, item) in &parsed.items {
            let mut item = item.clone();
            if let Some(folder) = item.folder_id
                && dropped.contains(&folder)
            {
                item.folder_id = None;
            }
            match engine.save_item(&vault_id, id, &item) {
                Ok(()) => {}
                Err(pwm_sync::SyncError::Vault(pwm_vault::VaultError::TooLarge)) => {
                    too_large += 1;
                    dropped.push(*id);
                    parsed.report.uncount(&item);
                }
                Err(err) => return Err(err.into()),
            }
        }
        parsed.report.leave_out(LeftOut::TooLarge, too_large);
        Ok(ImportPreview::from(&parsed.report))
    }

    /// Drops a prepared import without saving it.
    pub fn cancel_import(&mut self) {
        self.import = None;
    }
}
