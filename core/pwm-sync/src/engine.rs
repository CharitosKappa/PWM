//! The sync engine (D28): a state machine that asks the platform to send one
//! request at a time and applies each answer to the local cache.

use std::collections::{HashMap, HashSet, VecDeque};
use std::str::FromStr;

use pwm_crypto::{AccountKeyset, Envelope, Id, SymmetricKey};
use pwm_store::{Store, SyncState, VaultRecord};
use pwm_vault::{Item, ItemId, decrypt_item, encrypt_item_fitting};

use crate::SyncError;
use crate::apply::{apply_change, merge, move_to_copy, put, reseal, reupload_missing};
use crate::backoff::{backoff_delay_ms, parse_retry_after, random_fraction};
use crate::http::{HttpRequest, HttpResponse, Method};
use crate::tokens::Tokens;
use crate::wire::{self, b64, id_text};

/// Changes asked for per page (D27 allows up to 1000).
pub const DEFAULT_PAGE_LIMIT: u32 = 500;
/// Times one item is merged and resent within a run after 412s.
pub const MAX_CONFLICT_RETRIES: u8 = 3;

/// What the platform does next.
#[derive(Debug)]
pub enum Step {
    /// Send this request, then pass the answer to
    /// [`receive`](SyncEngine::receive), or call
    /// [`network_failed`](SyncEngine::network_failed).
    Send(HttpRequest),
    /// The run stopped on a network or server failure; start a new one
    /// after this long.
    Wait {
        delay_ms: u64,
    },
    Done(SyncOutcome),
}

#[derive(Debug)]
pub enum SyncOutcome {
    Synced(SyncReport),
    /// The refresh token was refused. Unsent changes are kept; sign in again.
    SignedOut,
    /// This device was revoked; the cache has been cleared.
    DeviceRevoked,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Changes received from the server.
    pub pulled: usize,
    /// Writes and deletes the server accepted.
    pub pushed: usize,
    /// Items changed on both sides (merged or kept side by side).
    pub conflicts: usize,
    /// Items the server refused; they stay queued.
    pub failed: usize,
    /// A vault was pulled again from the start because the server was
    /// behind this device.
    pub resynced: bool,
}

/// Things the platform must act on, collected with
/// [`take_events`](SyncEngine::take_events).
#[derive(Debug)]
pub enum SyncEvent {
    /// Store this pair in secure storage, replacing the old one.
    TokensRefreshed(Tokens),
    /// The server refused this item ("couldn't sync"); it stays queued.
    ItemFailed {
        vault_id: Id,
        item_id: ItemId,
        status: u16,
    },
}

pub struct SyncEngine<S> {
    store: S,
    keyset: AccountKeyset,
    tokens: Tokens,
    keys: HashMap<Id, SymmetricKey>,
    run: Option<Run>,
    /// Failed runs since the last good one, for backoff.
    failures: u32,
    page_limit: u32,
    events: Vec<SyncEvent>,
}

struct Run {
    task: Task,
    /// Waiting for the answer to a token refresh.
    refreshing: bool,
    /// The current request was already resent with refreshed tokens.
    refreshed: bool,
    report: SyncReport,
    /// A server epoch to record once every vault has been pulled (D30).
    new_epoch: Option<Id>,
}

enum Task {
    CreateVault {
        vault_id: Id,
        key: SymmetricKey,
        wrapped: Envelope,
    },
    ListVaults,
    Pull {
        vaults: Vec<Id>,
        index: usize,
        since: u64,
        /// Ids seen while pulling a vault again from 0 (D28 §9).
        seen: Option<HashSet<ItemId>>,
        /// The server was restored (new epoch): pull every vault from 0.
        resync_all: bool,
    },
    Push {
        queue: VecDeque<Pending>,
        current: Option<Sent>,
    },
}

#[derive(Clone, Copy)]
struct Pending {
    vault_id: Id,
    item_id: ItemId,
    attempts: u8,
}

/// What was in the request now in flight.
struct Sent {
    pending: Pending,
    revision: u64,
    envelope: Envelope,
    delete: bool,
}

impl<S: Store> SyncEngine<S> {
    /// An engine for an unlocked account. Vault keys are unwrapped from the
    /// cache; a vault whose key does not open is left out.
    pub fn new(store: S, keyset: AccountKeyset, tokens: Tokens) -> Result<Self, SyncError> {
        let mut keys = HashMap::new();
        for vault in store.vaults()? {
            if let Ok(key) = keyset.unwrap_vault_key(&vault.wrapped_key, &vault.vault_id) {
                keys.insert(vault.vault_id, key);
            }
        }
        Ok(Self {
            store,
            keyset,
            tokens,
            keys,
            run: None,
            failures: 0,
            page_limit: DEFAULT_PAGE_LIMIT,
            events: Vec::new(),
        })
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn into_store(self) -> S {
        self.store
    }

    /// Changes per page, 1 to 1000.
    pub fn set_page_limit(&mut self, limit: u32) {
        self.page_limit = limit.clamp(1, 1000);
    }

    pub fn take_events(&mut self) -> Vec<SyncEvent> {
        std::mem::take(&mut self.events)
    }

    fn key(&self, vault_id: &Id) -> Result<&SymmetricKey, SyncError> {
        self.keys.get(vault_id).ok_or(SyncError::UnknownVault)
    }

    // --- Local changes (D28 §3) ---

    /// The vault's items, without deleted ones.
    pub fn items(&self, vault_id: &Id) -> Result<Vec<(ItemId, Item)>, SyncError> {
        let key = self.key(vault_id)?;
        self.store
            .items(vault_id)?
            .into_iter()
            .filter(|r| r.state != SyncState::Deleted)
            .map(|r| {
                let item = decrypt_item(key, vault_id, &r.item_id, r.revision, &r.envelope)?;
                Ok((r.item_id, item))
            })
            .collect()
    }

    /// One item, or `None` if it does not exist or is deleted.
    pub fn item(&self, vault_id: &Id, item_id: &ItemId) -> Result<Option<Item>, SyncError> {
        let key = self.key(vault_id)?;
        match self.store.item(vault_id, item_id)? {
            Some(r) if r.state != SyncState::Deleted => Ok(Some(decrypt_item(
                key,
                vault_id,
                item_id,
                r.revision,
                &r.envelope,
            )?)),
            _ => Ok(None),
        }
    }

    /// Saves a new or edited item for upload.
    pub fn save_item(
        &mut self,
        vault_id: &Id,
        item_id: &ItemId,
        item: &Item,
    ) -> Result<(), SyncError> {
        let key = self.keys.get(vault_id).ok_or(SyncError::UnknownVault)?;
        let revision = match self.store.item(vault_id, item_id)? {
            None => 1,
            Some(r) => match r.state {
                SyncState::Synced => r.revision + 1,
                SyncState::Modified => r.revision,
                SyncState::Deleted => return Err(SyncError::ItemDeleted),
            },
        };
        let envelope = encrypt_item_fitting(key, vault_id, item_id, revision, &mut item.clone())?;
        put(
            &mut self.store,
            vault_id,
            item_id,
            revision,
            envelope,
            SyncState::Modified,
        )
    }

    /// Deletes an item: queued for upload, or simply removed if the server
    /// never had it.
    pub fn delete_item(&mut self, vault_id: &Id, item_id: &ItemId) -> Result<(), SyncError> {
        let Some(r) = self.store.item(vault_id, item_id)? else {
            return Ok(());
        };
        let base = match r.state {
            SyncState::Synced => r.revision,
            SyncState::Modified if r.revision == 1 => {
                self.store.delete_item(vault_id, item_id)?;
                return Ok(());
            }
            SyncState::Modified => r.revision - 1,
            SyncState::Deleted => return Ok(()),
        };
        put(
            &mut self.store,
            vault_id,
            item_id,
            base,
            r.envelope,
            SyncState::Deleted,
        )
    }

    /// Deletes items that have been in the trash for 30 days (D19); call at
    /// unlock. Returns how many.
    pub fn purge_trash(&mut self, now_ms: i64) -> Result<usize, SyncError> {
        let vaults: Vec<Id> = self.keys.keys().copied().collect();
        let mut purged = 0;
        for vault_id in vaults {
            for (item_id, item) in self.items(&vault_id)? {
                if item.is_due_for_purge(now_ms) {
                    self.delete_item(&vault_id, &item_id)?;
                    purged += 1;
                }
            }
        }
        Ok(purged)
    }

    // --- Runs ---

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    /// Abandons the current run; the answer to its last request is ignored.
    pub fn cancel(&mut self) {
        self.run = None;
    }

    /// Starts a sync run: vault list, pull, then push (D28 §4).
    pub fn start_sync(&mut self) -> Result<Step, SyncError> {
        self.start(Task::ListVaults)
    }

    /// Starts creating a new vault on the server (needs a connection);
    /// returns its id. It is cached once the server confirms it.
    pub fn start_create_vault(&mut self) -> Result<(Id, Step), SyncError> {
        if self.run.is_some() {
            return Err(SyncError::Busy);
        }
        let key = SymmetricKey::generate()?;
        let vault_id = *ItemId::generate()?.as_bytes();
        let wrapped = self.keyset.wrap_vault_key(&key, &vault_id)?;
        let step = self.start(Task::CreateVault {
            vault_id,
            key,
            wrapped,
        })?;
        Ok((vault_id, step))
    }

    fn start(&mut self, task: Task) -> Result<Step, SyncError> {
        if self.run.is_some() {
            return Err(SyncError::Busy);
        }
        let mut run = Run {
            task,
            refreshing: false,
            refreshed: false,
            report: SyncReport::default(),
            new_epoch: None,
        };
        let step = self.request(&mut run);
        self.settle(run, step)
    }

    /// Takes the answer to the request from the last [`Step::Send`].
    pub fn receive(&mut self, response: HttpResponse, now_ms: i64) -> Result<Step, SyncError> {
        let mut run = self.run.take().ok_or(SyncError::NotRunning)?;
        let step = self.on_response(&mut run, &response, now_ms);
        self.settle(run, step)
    }

    /// The request from the last [`Step::Send`] got no answer.
    pub fn network_failed(&mut self) -> Result<Step, SyncError> {
        self.run.take().ok_or(SyncError::NotRunning)?;
        Ok(self.wait(None))
    }

    /// Keeps the run only while it has a request in flight.
    fn settle(&mut self, run: Run, step: Result<Step, SyncError>) -> Result<Step, SyncError> {
        let step = step?;
        if matches!(step, Step::Send(_)) {
            self.run = Some(run);
        }
        Ok(step)
    }

    fn wait(&mut self, retry_after_ms: Option<u64>) -> Step {
        let delay_ms = backoff_delay_ms(self.failures, random_fraction(), retry_after_ms);
        self.failures = self.failures.saturating_add(1);
        Step::Wait { delay_ms }
    }

    fn done(&mut self, outcome: SyncOutcome) -> Step {
        if matches!(outcome, SyncOutcome::Synced(_)) {
            self.failures = 0;
        }
        Step::Done(outcome)
    }

    fn authorized(&self, request: HttpRequest) -> HttpRequest {
        request.header_value("authorization", format!("Bearer {}", self.tokens.access()))
    }

    fn json<T: serde::Serialize>(request: HttpRequest, body: &T) -> HttpRequest {
        request.json(serde_json::to_vec(body).expect("plain string fields always serialize"))
    }

    /// The request for where `run` is now, or the end of the run.
    fn request(&mut self, run: &mut Run) -> Result<Step, SyncError> {
        loop {
            match &mut run.task {
                Task::CreateVault {
                    vault_id, wrapped, ..
                } => {
                    let body = wire::NewVault {
                        id: id_text(vault_id),
                        wrapped_key: b64(&wrapped.to_bytes()),
                    };
                    let request = HttpRequest::new(Method::Post, "/v1/vaults".to_owned());
                    return Ok(Step::Send(Self::json(self.authorized(request), &body)));
                }
                Task::ListVaults => {
                    let request = HttpRequest::new(Method::Get, "/v1/vaults".to_owned());
                    return Ok(Step::Send(self.authorized(request)));
                }
                Task::Pull {
                    vaults,
                    index,
                    since,
                    ..
                } => {
                    if let Some(vault_id) = vaults.get(*index) {
                        let path = format!(
                            "/v1/vaults/{}/items?since={since}&limit={}",
                            id_text(vault_id),
                            self.page_limit
                        );
                        return Ok(Step::Send(
                            self.authorized(HttpRequest::new(Method::Get, path)),
                        ));
                    }
                    // Every vault is pulled: the restore (if any) is handled.
                    if let Some(epoch) = run.new_epoch.take() {
                        self.store.set_server_epoch(&epoch)?;
                    }
                    let queue = self
                        .store
                        .pending_items()?
                        .into_iter()
                        .filter(|r| self.keys.contains_key(&r.vault_id))
                        .map(|r| Pending {
                            vault_id: r.vault_id,
                            item_id: r.item_id,
                            attempts: 0,
                        })
                        .collect();
                    run.task = Task::Push {
                        queue,
                        current: None,
                    };
                }
                Task::Push { queue, current } => {
                    // After a token refresh the current item is sent again.
                    let Some(pending) = current
                        .take()
                        .map(|s| s.pending)
                        .or_else(|| queue.pop_front())
                    else {
                        let report = std::mem::take(&mut run.report);
                        return Ok(self.done(SyncOutcome::Synced(report)));
                    };
                    let Some(record) = self.store.item(&pending.vault_id, &pending.item_id)? else {
                        continue;
                    };
                    let path = format!(
                        "/v1/vaults/{}/items/{}",
                        id_text(&pending.vault_id),
                        pending.item_id
                    );
                    let request = match record.state {
                        SyncState::Synced => continue,
                        SyncState::Modified => {
                            let request = HttpRequest::new(Method::Put, path);
                            let request = if record.revision == 1 {
                                request.header_value("if-none-match", "*".to_owned())
                            } else {
                                request.header_value(
                                    "if-match",
                                    format!("\"{}\"", record.revision - 1),
                                )
                            };
                            let body = wire::ItemWrite {
                                revision: record.revision.to_string(),
                                blob: b64(&record.envelope.to_bytes()),
                            };
                            Self::json(request, &body)
                        }
                        SyncState::Deleted => HttpRequest::new(Method::Delete, path)
                            .header_value("if-match", format!("\"{}\"", record.revision)),
                    };
                    *current = Some(Sent {
                        pending,
                        revision: record.revision,
                        envelope: record.envelope,
                        delete: record.state == SyncState::Deleted,
                    });
                    return Ok(Step::Send(self.authorized(request)));
                }
            }
        }
    }

    fn refresh_request(&self) -> HttpRequest {
        let body = wire::Refresh {
            refresh_token: self.tokens.refresh(),
        };
        Self::json(
            HttpRequest::new(Method::Post, "/v1/auth/refresh".to_owned()),
            &body,
        )
    }

    fn on_response(
        &mut self,
        run: &mut Run,
        response: &HttpResponse,
        now_ms: i64,
    ) -> Result<Step, SyncError> {
        if run.refreshing {
            return self.on_refresh(run, response);
        }
        match response.status {
            401 if run.refreshed => return Ok(self.done(SyncOutcome::SignedOut)),
            401 => {
                run.refreshing = true;
                return Ok(Step::Send(self.refresh_request()));
            }
            403 if is_revoked(response) => return self.revoked(),
            429 | 500..=599 => {
                return Ok(self.wait(parse_retry_after(response.header("retry-after"))));
            }
            _ => {}
        }
        run.refreshed = false;
        match run.task {
            Task::CreateVault { .. } => self.on_vault_created(run, response),
            Task::ListVaults => self.on_vault_list(run, response),
            Task::Pull { .. } => self.on_page(run, response, now_ms),
            Task::Push { .. } => self.on_pushed(run, response, now_ms),
        }
    }

    fn on_refresh(&mut self, run: &mut Run, response: &HttpResponse) -> Result<Step, SyncError> {
        run.refreshing = false;
        match response.status {
            200 => {
                let Ok(pair) = serde_json::from_slice::<wire::TokenPair>(&response.body) else {
                    return Ok(self.wait(None));
                };
                self.tokens = Tokens::new(pair.access_token, pair.refresh_token);
                self.events
                    .push(SyncEvent::TokensRefreshed(self.tokens.duplicate()));
                run.refreshed = true;
                self.request(run)
            }
            403 if is_revoked(response) => self.revoked(),
            429 | 500..=599 => Ok(self.wait(parse_retry_after(response.header("retry-after")))),
            _ => Ok(self.done(SyncOutcome::SignedOut)),
        }
    }

    /// This device was revoked: nothing of the account stays on it (D20).
    fn revoked(&mut self) -> Result<Step, SyncError> {
        self.store.clear()?;
        self.keys.clear();
        Ok(self.done(SyncOutcome::DeviceRevoked))
    }

    fn on_vault_created(
        &mut self,
        run: &mut Run,
        response: &HttpResponse,
    ) -> Result<Step, SyncError> {
        let task = std::mem::replace(&mut run.task, Task::ListVaults);
        let Task::CreateVault {
            vault_id,
            key,
            wrapped,
        } = task
        else {
            unreachable!("checked by the caller");
        };
        if response.status != 201 {
            return Ok(self.wait(None));
        }
        self.store.put_vault(&VaultRecord {
            vault_id,
            wrapped_key: wrapped,
            sync_cursor: 0,
        })?;
        self.keys.insert(vault_id, key);
        Ok(self.done(SyncOutcome::Synced(SyncReport::default())))
    }

    /// Mirrors the server's vault list: new vaults start at cursor 0, keys
    /// are updated, vaults no longer listed are removed.
    fn on_vault_list(&mut self, run: &mut Run, response: &HttpResponse) -> Result<Step, SyncError> {
        let parsed = (response.status == 200)
            .then(|| wire::vault_list(&response.body))
            .flatten();
        let Some((listed, own)) = parsed else {
            return Ok(self.wait(None));
        };
        let cached: HashMap<Id, VaultRecord> = self
            .store
            .vaults()?
            .into_iter()
            .map(|v| (v.vault_id, v))
            .collect();
        for vault in own {
            let Ok(key) = self.keyset.unwrap_vault_key(&vault.wrapped_key, &vault.id) else {
                continue;
            };
            let cursor = cached.get(&vault.id).map(|v| v.sync_cursor);
            if cached.get(&vault.id).map(|v| &v.wrapped_key) != Some(&vault.wrapped_key) {
                self.store.put_vault(&VaultRecord {
                    vault_id: vault.id,
                    wrapped_key: vault.wrapped_key,
                    sync_cursor: cursor.unwrap_or(0),
                })?;
            }
            self.keys.insert(vault.id, key);
        }
        for vault_id in cached.keys().filter(|id| !listed.contains(id)) {
            self.store.delete_vault(vault_id)?;
            self.keys.remove(vault_id);
        }
        // A new server epoch means the database was restored from a backup:
        // seqs no longer say what changed, so every vault is pulled from 0
        // and what the server lost goes up again. The first epoch seen is
        // just recorded.
        let epoch = response
            .header("pwm-epoch")
            .and_then(|e| ItemId::from_str(e).ok())
            .map(|e| *e.as_bytes());
        let stored = self.store.server_epoch()?;
        let resync_all = matches!((stored, epoch), (Some(old), Some(new)) if old != new);
        if epoch.is_some() && epoch != stored {
            run.new_epoch = epoch;
        }
        run.report.resynced |= resync_all;
        let mut vaults: Vec<Id> = self.keys.keys().copied().collect();
        vaults.sort_unstable();
        let since = match vaults.first() {
            Some(id) if !resync_all => self.cursor(id)?,
            _ => 0,
        };
        run.task = Task::Pull {
            vaults,
            index: 0,
            since,
            seen: resync_all.then(HashSet::new),
            resync_all,
        };
        self.request(run)
    }

    fn cursor(&self, vault_id: &Id) -> Result<u64, SyncError> {
        Ok(self
            .store
            .vaults()?
            .into_iter()
            .find(|v| v.vault_id == *vault_id)
            .map_or(0, |v| v.sync_cursor))
    }

    fn set_cursor(&mut self, vault_id: &Id, cursor: u64) -> Result<(), SyncError> {
        if let Some(mut vault) = self
            .store
            .vaults()?
            .into_iter()
            .find(|v| v.vault_id == *vault_id)
        {
            vault.sync_cursor = cursor;
            self.store.put_vault(&vault)?;
        }
        Ok(())
    }

    fn on_page(
        &mut self,
        run: &mut Run,
        response: &HttpResponse,
        now_ms: i64,
    ) -> Result<Step, SyncError> {
        let Run { task, report, .. } = run;
        let Task::Pull {
            vaults,
            index,
            since,
            seen,
            resync_all,
        } = task
        else {
            unreachable!("checked by the caller");
        };
        let vault_id = vaults[*index];
        let mut finished = response.status == 404;
        if response.status == 200 {
            let Some(page) = wire::page(&response.body) else {
                return Ok(self.wait(None));
            };
            // The server is behind this device: pull the vault again from 0.
            if *since > 0 && page.next < *since && seen.is_none() {
                self.set_cursor(&vault_id, 0)?;
                report.resynced = true;
                *seen = Some(HashSet::new());
                *since = 0;
                return self.request(run);
            }
            let key = self.keys.get(&vault_id).ok_or(SyncError::UnknownVault)?;
            for change in page.changes {
                if let Some(seen) = seen.as_mut() {
                    seen.insert(change.id);
                }
                let applied = apply_change(
                    &mut self.store,
                    key,
                    &vault_id,
                    &change.id,
                    change.revision,
                    change.envelope,
                    now_ms,
                )?;
                report.pulled += 1;
                report.conflicts += usize::from(applied.conflict);
            }
            // Saved after the items, so a crash repeats only this page.
            self.set_cursor(&vault_id, page.next)?;
            *since = page.next;
            if !page.more {
                if let Some(seen) = seen.take() {
                    let key = self.keys.get(&vault_id).ok_or(SyncError::UnknownVault)?;
                    reupload_missing(&mut self.store, key, &vault_id, &seen)?;
                }
                finished = true;
            }
        } else if !finished {
            return Ok(self.wait(None));
        }
        if finished {
            *index += 1;
            *seen = resync_all.then(HashSet::new);
            if let Some(next) = vaults.get(*index).copied() {
                *since = if *resync_all { 0 } else { self.cursor(&next)? };
            }
        }
        self.request(run)
    }

    /// After our own write got `seq`: if it directly follows the cursor,
    /// nothing is missing in between (D27 commits in seq order), so the
    /// next pull need not fetch it again.
    fn advance_cursor(&mut self, vault_id: &Id, seq: Option<u64>) -> Result<(), SyncError> {
        if let Some(seq) = seq
            && self.cursor(vault_id)? + 1 == seq
        {
            self.set_cursor(vault_id, seq)?;
        }
        Ok(())
    }

    fn on_pushed(
        &mut self,
        run: &mut Run,
        response: &HttpResponse,
        now_ms: i64,
    ) -> Result<Step, SyncError> {
        let Task::Push { queue, current } = &mut run.task else {
            unreachable!("checked by the caller");
        };
        let Some(sent) = current.take() else {
            return self.request(run);
        };
        let Pending {
            vault_id,
            item_id,
            attempts,
        } = sent.pending;
        let key = self.keys.get(&vault_id).ok_or(SyncError::UnknownVault)?;
        let local = self.store.item(&vault_id, &item_id)?;
        // A deleted record cannot be edited, so it is still as sent.
        let deleting = sent.delete;
        let mut retry = false;
        let mut failed = None;
        match response.status {
            200 | 201 => {
                let seq = wire::written_seq(&response.body);
                if deleting {
                    self.store.delete_item(&vault_id, &item_id)?;
                } else {
                    uploaded(&mut self.store, key, &vault_id, &item_id, &sent, local)?;
                }
                run.report.pushed += 1;
                self.advance_cursor(&vault_id, seq)?;
            }
            412 => match (wire::conflict(&response.body), local) {
                (None, _) => return Ok(self.wait(None)),
                // Our own write, sent again after a lost answer.
                (Some((_, envelope)), local) if envelope == sent.envelope && !deleting => {
                    uploaded(&mut self.store, key, &vault_id, &item_id, &sent, local)?;
                }
                (Some((revision, envelope)), Some(l)) if l.state == SyncState::Modified => {
                    run.report.conflicts += 1;
                    if let Some(copy) = merge(&mut self.store, key, &l, revision, envelope, now_ms)?
                    {
                        queue.push_back(Pending {
                            vault_id,
                            item_id: copy,
                            attempts: 0,
                        });
                    }
                    retry = true;
                }
                // A delete (or an item gone meanwhile) against a newer edit:
                // the edit wins.
                (Some((revision, envelope)), _) => {
                    put(
                        &mut self.store,
                        &vault_id,
                        &item_id,
                        revision,
                        envelope,
                        SyncState::Synced,
                    )?;
                }
            },
            409 | 404 if deleting => self.store.delete_item(&vault_id, &item_id)?,
            409 => {
                if let Some(l) = local.filter(|l| l.state == SyncState::Modified) {
                    run.report.conflicts += 1;
                    if let Some(copy) = move_to_copy(&mut self.store, key, &l)? {
                        queue.push_back(Pending {
                            vault_id,
                            item_id: copy,
                            attempts: 0,
                        });
                    }
                } else {
                    self.store.delete_item(&vault_id, &item_id)?;
                }
            }
            // The server lost the item (restore): upload it again as new.
            404 if sent.revision > 1 => {
                if let Some(l) = local.filter(|l| l.state == SyncState::Modified) {
                    reseal(&mut self.store, key, &l, 1)?;
                    retry = true;
                }
            }
            status => failed = Some(status),
        }
        if retry {
            if attempts + 1 < MAX_CONFLICT_RETRIES {
                queue.push_front(Pending {
                    vault_id,
                    item_id,
                    attempts: attempts + 1,
                });
            } else {
                failed = Some(response.status);
            }
        }
        if let Some(status) = failed {
            run.report.failed += 1;
            self.events.push(SyncEvent::ItemFailed {
                vault_id,
                item_id,
                status,
            });
        }
        self.request(run)
    }
}

fn is_revoked(response: &HttpResponse) -> bool {
    wire::error_code(&response.body).as_deref() == Some("device_revoked")
}

/// The server accepted `sent`. The cache moves to `Synced` unless the user
/// changed the item while the request was in flight.
fn uploaded<S: Store>(
    store: &mut S,
    key: &SymmetricKey,
    vault_id: &Id,
    item_id: &ItemId,
    sent: &Sent,
    local: Option<pwm_store::ItemRecord>,
) -> Result<(), SyncError> {
    match local {
        Some(l) if l.state == SyncState::Modified && l.revision == sent.revision => {
            if l.envelope == sent.envelope {
                put(
                    store,
                    vault_id,
                    item_id,
                    l.revision,
                    l.envelope,
                    SyncState::Synced,
                )
            } else {
                // Edited again meanwhile: the new content builds on what we sent.
                reseal(store, key, &l, sent.revision + 1)
            }
        }
        // Deleted meanwhile: delete what the server now has.
        Some(l) if l.state == SyncState::Deleted => put(
            store,
            vault_id,
            item_id,
            sent.revision,
            l.envelope,
            SyncState::Deleted,
        ),
        None => put(
            store,
            vault_id,
            item_id,
            sent.revision,
            sent.envelope.clone(),
            SyncState::Deleted,
        ),
        Some(_) => Ok(()),
    }
}
