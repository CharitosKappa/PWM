//! The client: one object per signed-in device, locked or unlocked, that
//! runs sign-up, login and sync as steps (D28, D31) and edits items given as
//! D18 JSON.

use pwm_crypto::{AccountKeyset, Id, KdfParams, Secret};
use pwm_store::{AccountRecord, Store};
use pwm_sync::{HttpResponse, SyncEngine, SyncEvent, SyncOutcome, SyncReport, Tokens};
use pwm_vault::{Item, ItemId};

use crate::auth::{self, AuthFlow, Finished, Next};
use crate::{ClientError, Device, Platform};

/// What the platform does next.
#[derive(Debug)]
pub enum Step {
    /// Send this request, then call [`Client::receive`] or
    /// [`Client::network_failed`].
    Send(pwm_sync::HttpRequest),
    /// A sync run stopped on a failure; start a new one after this long.
    Wait {
        delay_ms: u64,
    },
    Done(Outcome),
}

#[derive(Debug)]
pub enum Outcome {
    /// Show the Secret Key on the Emergency Kit now; it is not shown again.
    /// Then create the first vault with [`Client::start_create_vault`].
    SignedUp {
        secret_key: Secret<String>,
    },
    LoggedIn,
    VaultCreated {
        vault_id: String,
    },
    Synced(SyncReport),
    /// The refresh token was refused: log in again. Unsent changes are kept.
    SignedOut,
    /// This device was revoked: its cache is cleared and the client locked.
    DeviceRevoked,
    /// The account's devices, from [`Client::start_list_devices`].
    Devices(Vec<crate::DeviceInfo>),
    /// A device was renamed or revoked.
    DeviceUpdated,
    /// The account's open device approval requests (D46).
    Approvals(Vec<crate::ApprovalRequest>),
    /// An approval was accepted, completed or denied.
    ApprovalUpdated,
}

enum State<S> {
    Locked(S),
    Unlocked(Box<SyncEngine<S>>),
    /// Only while moving between the two.
    Moving,
}

pub(crate) enum Running {
    Idle,
    Auth(AuthFlow),
    Sync,
    CreateVault(Id),
    /// Sync, then create a vault if the account has none (D34).
    EnsureVault,
    /// A device request (D43 §6).
    Devices,
    /// A device approval call (D46).
    Approval(crate::approval::ApprovalCall),
}

pub struct Client<S: Store> {
    state: State<S>,
    pub(crate) running: Running,
    events: Vec<SyncEvent>,
    signup_params: KdfParams,
    pub(crate) autofill: crate::autofill::AutofillState,
    /// The master password was entered again for an export (D24).
    pub(crate) export_grant: Option<pwm_export::ExportGrant>,
    /// The approval this device accepted and has not finished (D46).
    pub(crate) approving: Option<crate::approval::Approving>,
    /// The commitments of the requests last listed, by id.
    pub(crate) approval_commitments: std::collections::HashMap<Id, [u8; 32]>,
}

pub(crate) fn parse_id(text: &str) -> Result<Id, ClientError> {
    text.parse::<ItemId>()
        .map(|id| *id.as_bytes())
        .map_err(|_| ClientError::InvalidId)
}

pub(crate) fn parse_item_id(text: &str) -> Result<ItemId, ClientError> {
    text.parse::<ItemId>().map_err(|_| ClientError::InvalidId)
}

/// A new random id for an item or a device.
pub fn new_id() -> Result<String, ClientError> {
    Ok(ItemId::generate()?.to_string())
}

impl<S: Store> Client<S> {
    /// A locked client over `store` (the device's cache).
    pub fn new(store: S) -> Self {
        Self {
            state: State::Locked(store),
            running: Running::Idle,
            events: Vec::new(),
            signup_params: KdfParams::DEFAULT,
            autofill: crate::autofill::AutofillState::default(),
            export_grant: None,
            approving: None,
            approval_commitments: std::collections::HashMap::new(),
        }
    }

    /// KDF params for new accounts: [`KdfParams::DEFAULT`], or the mobile
    /// floor on a device that cannot spare 64 MiB (D15).
    pub fn set_signup_params(&mut self, params: KdfParams) -> Result<(), ClientError> {
        params.validate().map_err(|_| ClientError::Internal)?;
        self.signup_params = params;
        Ok(())
    }

    pub fn is_unlocked(&self) -> bool {
        matches!(self.state, State::Unlocked(_))
    }

    /// The cache, for persisting a `MemoryStore` snapshot.
    pub fn store(&self) -> &S {
        match &self.state {
            State::Locked(store) => store,
            State::Unlocked(engine) => engine.store(),
            State::Moving => unreachable!("never observable"),
        }
    }

    /// The account cached on this device, if any (its id as a UUID string).
    pub fn account_id(&self) -> Result<Option<String>, ClientError> {
        Ok(self
            .store()
            .account()?
            .map(|a| auth::id_text(&a.account_id)))
    }

    /// New tokens to keep in secure storage, and items the server refused.
    pub fn take_events(&mut self) -> Vec<SyncEvent> {
        let mut events = std::mem::take(&mut self.events);
        if let State::Unlocked(engine) = &mut self.state {
            events.extend(engine.take_events());
        }
        events
    }

    pub(crate) fn engine(&mut self) -> Result<&mut SyncEngine<S>, ClientError> {
        match &mut self.state {
            State::Unlocked(engine) => Ok(engine),
            _ => Err(ClientError::Locked),
        }
    }

    pub(crate) fn engine_ref(&self) -> Result<&SyncEngine<S>, ClientError> {
        match &self.state {
            State::Unlocked(engine) => Ok(engine),
            _ => Err(ClientError::Locked),
        }
    }

    pub(crate) fn idle(&self) -> Result<(), ClientError> {
        match self.running {
            Running::Idle => Ok(()),
            _ => Err(ClientError::Busy),
        }
    }

    // --- Account ---

    /// Starts creating an account: the keys are made now (the KDF takes a
    /// moment), then the server is asked to register it. The master password
    /// must score at least 3 (D21).
    pub fn start_signup(
        &mut self,
        email: &str,
        password: &str,
        device_id: &str,
        platform: Platform,
        device_name: &str,
    ) -> Result<Step, ClientError> {
        self.idle()?;
        let strength = pwm_generator::strength(password, &[email]);
        if strength.score < pwm_generator::MIN_SIGNUP_SCORE {
            return Err(ClientError::WeakPassword);
        }
        let device = Device {
            id: parse_id(device_id)?,
            platform,
            name: device_name.to_owned(),
        };
        let (flow, request) = auth::start_signup(email, password, device, self.signup_params)?;
        self.running = Running::Auth(flow);
        Ok(Step::Send(request))
    }

    /// Starts logging in on this device with the email, master password and
    /// Secret Key. `device_id` stays the same for this install. Once logged
    /// in, the device sends its name, sealed (D43 §6); an empty name sends
    /// none.
    pub fn start_login(
        &mut self,
        email: &str,
        password: &str,
        secret_key: &str,
        device_id: &str,
        platform: Platform,
        device_name: &str,
    ) -> Result<Step, ClientError> {
        self.idle()?;
        let device = Device {
            id: parse_id(device_id)?,
            platform,
            name: device_name.to_owned(),
        };
        let (flow, request) = auth::start_login(email, password, secret_key, device)?;
        self.running = Running::Auth(flow);
        Ok(Step::Send(request))
    }

    /// Unlocks the account cached on this device, offline. `tokens` come
    /// from secure storage.
    pub fn unlock(
        &mut self,
        password: &str,
        secret_key: &str,
        tokens: Tokens,
    ) -> Result<(), ClientError> {
        self.idle()?;
        let State::Locked(store) = &self.state else {
            return Ok(());
        };
        let record = store.account()?.ok_or(ClientError::WrongPassword)?;
        let keyset = auth::unlock(&record, password, secret_key)?;
        self.open(keyset, tokens)
    }

    /// Drops every key from memory. A running operation is abandoned.
    pub fn lock(&mut self) {
        self.running = Running::Idle;
        self.export_grant = None;
        self.approving = None;
        self.approval_commitments.clear();
        if let State::Unlocked(_) = self.state {
            let State::Unlocked(engine) = std::mem::replace(&mut self.state, State::Moving) else {
                unreachable!("checked above");
            };
            self.state = State::Locked(engine.into_store());
        }
    }

    /// Locks and deletes the cached account, its vaults and items, so the
    /// device keeps nothing of it. The platform deletes its stored tokens and
    /// Secret Key; the device id stays.
    pub fn sign_out(&mut self) -> Result<(), ClientError> {
        self.lock();
        let State::Locked(store) = &mut self.state else {
            unreachable!("locked just now");
        };
        store.clear()?;
        Ok(())
    }

    /// Unlocks with an already opened keyset (session restore, D34).
    pub(crate) fn open_unlocked(
        &mut self,
        keyset: AccountKeyset,
        tokens: Tokens,
    ) -> Result<(), ClientError> {
        self.idle()?;
        self.open(keyset, tokens)
    }

    fn open(&mut self, keyset: AccountKeyset, tokens: Tokens) -> Result<(), ClientError> {
        let state = std::mem::replace(&mut self.state, State::Moving);
        let store = match state {
            State::Locked(store) => store,
            State::Unlocked(engine) => engine.into_store(),
            State::Moving => unreachable!("never observable"),
        };
        match SyncEngine::new(store, keyset, tokens) {
            Ok(engine) => {
                self.state = State::Unlocked(Box::new(engine));
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Stores the account (clearing a different one's cache first) and
    /// unlocks with its keyset.
    fn finish_auth(
        &mut self,
        record: AccountRecord,
        keyset: AccountKeyset,
        tokens: Tokens,
    ) -> Result<(), ClientError> {
        self.lock();
        let State::Locked(store) = &mut self.state else {
            unreachable!("locked just now");
        };
        if store
            .account()?
            .is_some_and(|a| a.account_id != record.account_id)
        {
            store.clear()?;
        }
        store.put_account(&record)?;
        self.events.push(SyncEvent::TokensRefreshed(Tokens::new(
            tokens.access().to_owned(),
            tokens.refresh().to_owned(),
        )));
        self.open(keyset, tokens)
    }

    // --- Server conversations ---

    /// Starts a sync run (vault list, pull, push).
    pub fn start_sync(&mut self) -> Result<Step, ClientError> {
        self.idle()?;
        let step = self.engine()?.start_sync()?;
        self.running = Running::Sync;
        self.map_engine_step(step)
    }

    /// Syncs, then creates a vault only if the account has none, so a
    /// retried first-vault step never leaves an extra one. Ends with
    /// `VaultCreated`, or `Synced` when a vault already existed.
    pub fn start_ensure_vault(&mut self) -> Result<Step, ClientError> {
        self.idle()?;
        let step = self.engine()?.start_sync()?;
        self.running = Running::EnsureVault;
        self.map_engine_step(step)
    }

    /// Lists the account's devices with their names opened; needs a
    /// connection. Ends with `Devices`.
    pub fn start_list_devices(&mut self) -> Result<Step, ClientError> {
        self.start_device_task(pwm_sync::DeviceTask::List)
    }

    /// Renames one of the account's devices (this one included); ends with
    /// `DeviceUpdated`, or `unknown_device`.
    pub fn start_rename_device(
        &mut self,
        device_id: &str,
        name: &str,
    ) -> Result<Step, ClientError> {
        let device = parse_id(device_id)?;
        let account = self.account_record()?.account_id;
        let name_sealed = self
            .engine_ref()?
            .keyset()
            .seal_device_name(&account, &device, name.trim())
            .map_err(|_| ClientError::InvalidDeviceName)?;
        self.start_device_task(pwm_sync::DeviceTask::Rename {
            device_id: device,
            name_sealed,
        })
    }

    /// Revokes another device: its session ends and it clears its cache at
    /// its next request (D26). Ends with `DeviceUpdated`. To leave this
    /// device, sign out instead.
    pub fn start_revoke_device(&mut self, device_id: &str) -> Result<Step, ClientError> {
        let device_id = parse_id(device_id)?;
        self.start_device_task(pwm_sync::DeviceTask::Revoke { device_id })
    }

    fn start_device_task(&mut self, task: pwm_sync::DeviceTask) -> Result<Step, ClientError> {
        self.idle()?;
        let step = self.engine()?.start_device_task(task)?;
        self.running = Running::Devices;
        self.map_engine_step(step)
    }

    /// Opens each device name; one that does not open (moved from another
    /// device, or corrupted) is `None`.
    fn open_devices(
        &self,
        records: Vec<pwm_sync::DeviceRecord>,
    ) -> Result<Vec<crate::DeviceInfo>, ClientError> {
        let account = self.account_record()?.account_id;
        let keyset = self.engine_ref()?.keyset();
        Ok(records
            .into_iter()
            .map(|r| crate::DeviceInfo {
                id: auth::id_text(&r.id),
                name: r
                    .name_sealed
                    .as_ref()
                    .and_then(|sealed| keyset.open_device_name(sealed, &account, &r.id).ok()),
                platform: r.platform,
                created_at: r.created_at,
                last_seen_at: r.last_seen_at,
                revoked_at: r.revoked_at,
                current: r.current,
            })
            .collect())
    }

    pub(crate) fn account_record(&self) -> Result<AccountRecord, ClientError> {
        self.engine_ref()?
            .store()
            .account()?
            .ok_or(ClientError::Locked)
    }

    /// Creates a vault on the server; needs a connection.
    pub fn start_create_vault(&mut self) -> Result<Step, ClientError> {
        self.idle()?;
        let (vault_id, step) = self.engine()?.start_create_vault()?;
        self.running = Running::CreateVault(vault_id);
        self.map_engine_step(step)
    }

    /// The answer to the request from the last [`Step::Send`].
    pub fn receive(&mut self, response: HttpResponse, now_ms: i64) -> Result<Step, ClientError> {
        match std::mem::replace(&mut self.running, Running::Idle) {
            Running::Idle => Err(ClientError::NotRunning),
            Running::Auth(flow) => match auth::advance(flow, &response)? {
                Next::Send(flow, request) => {
                    self.running = Running::Auth(flow);
                    Ok(Step::Send(request))
                }
                Next::Done(done) => match *done {
                    Finished::SignedUp(record, keyset, tokens, secret_key) => {
                        self.finish_auth(record, keyset, tokens)?;
                        Ok(Step::Done(Outcome::SignedUp { secret_key }))
                    }
                    Finished::LoggedIn(record, keyset, tokens) => {
                        self.finish_auth(record, keyset, tokens)?;
                        Ok(Step::Done(Outcome::LoggedIn))
                    }
                },
            },
            running => {
                self.running = running;
                let step = self.engine()?.receive(response, now_ms);
                self.after_engine(step)
            }
        }
    }

    /// The request from the last [`Step::Send`] got no answer.
    pub fn network_failed(&mut self) -> Result<Step, ClientError> {
        match std::mem::replace(&mut self.running, Running::Idle) {
            Running::Idle => Err(ClientError::NotRunning),
            Running::Auth(flow) => match auth::after_network_failure(flow) {
                Some(Finished::LoggedIn(record, keyset, tokens)) => {
                    self.finish_auth(record, keyset, tokens)?;
                    Ok(Step::Done(Outcome::LoggedIn))
                }
                _ => Err(ClientError::Network),
            },
            running => {
                self.running = running;
                let step = self.engine()?.network_failed();
                self.after_engine(step)
            }
        }
    }

    /// Abandons the running operation.
    pub fn cancel(&mut self) {
        self.running = Running::Idle;
        if let State::Unlocked(engine) = &mut self.state {
            engine.cancel();
        }
    }

    fn after_engine(
        &mut self,
        step: Result<pwm_sync::Step, pwm_sync::SyncError>,
    ) -> Result<Step, ClientError> {
        match step {
            Ok(step) => self.map_engine_step(step),
            Err(err) => {
                self.running = Running::Idle;
                Err(err.into())
            }
        }
    }

    pub(crate) fn map_engine_step(&mut self, step: pwm_sync::Step) -> Result<Step, ClientError> {
        let step = match step {
            pwm_sync::Step::Send(request) => return Ok(Step::Send(request)),
            pwm_sync::Step::Wait { delay_ms } => Step::Wait { delay_ms },
            pwm_sync::Step::Done(SyncOutcome::Synced(report)) => match self.running {
                Running::CreateVault(vault_id) => Step::Done(Outcome::VaultCreated {
                    vault_id: auth::id_text(&vault_id),
                }),
                Running::EnsureVault if self.vault_ids()?.is_empty() => {
                    let (vault_id, step) = self.engine()?.start_create_vault()?;
                    self.running = Running::CreateVault(vault_id);
                    return self.map_engine_step(step);
                }
                _ => Step::Done(Outcome::Synced(report)),
            },
            pwm_sync::Step::Done(SyncOutcome::SignedOut) => Step::Done(Outcome::SignedOut),
            pwm_sync::Step::Done(SyncOutcome::Devices(records)) => {
                Step::Done(Outcome::Devices(self.open_devices(records)?))
            }
            pwm_sync::Step::Done(SyncOutcome::DeviceUpdated) => Step::Done(Outcome::DeviceUpdated),
            pwm_sync::Step::Done(SyncOutcome::Answered { status, body }) => {
                let Running::Approval(call) = self.running else {
                    self.running = Running::Idle;
                    return Err(ClientError::Protocol);
                };
                self.running = Running::Idle;
                return Ok(Step::Done(self.approval_answer(call, status, &body)?));
            }
            pwm_sync::Step::Done(SyncOutcome::DeviceRevoked) => {
                // The engine cleared the cache; nothing stays unlocked either.
                self.running = Running::Idle;
                self.lock();
                return Ok(Step::Done(Outcome::DeviceRevoked));
            }
        };
        self.running = Running::Idle;
        Ok(step)
    }

    // --- Vaults and items (D18 JSON with an "id" field) ---

    pub fn vault_ids(&self) -> Result<Vec<String>, ClientError> {
        let engine = self.engine_ref()?;
        let mut ids: Vec<String> = engine
            .store()
            .vaults()?
            .iter()
            .map(|v| auth::id_text(&v.vault_id))
            .collect();
        ids.sort();
        Ok(ids)
    }

    /// The vault's items (not deleted ones), each as JSON with its id.
    pub fn items(&self, vault_id: &str) -> Result<Vec<Secret<String>>, ClientError> {
        let vault_id = parse_id(vault_id)?;
        self.engine_ref()?
            .items(&vault_id)?
            .iter()
            .map(|(id, item)| Ok(item.to_json_with_id(id)?))
            .collect()
    }

    pub fn item(
        &self,
        vault_id: &str,
        item_id: &str,
    ) -> Result<Option<Secret<String>>, ClientError> {
        let (vault_id, item_id) = (parse_id(vault_id)?, parse_item_id(item_id)?);
        match self.engine_ref()?.item(&vault_id, &item_id)? {
            Some(item) => Ok(Some(item.to_json_with_id(&item_id)?)),
            None => Ok(None),
        }
    }

    /// Saves an item given as JSON with its id: new if the id is unknown,
    /// otherwise an edit, which keeps the previous version in history (D19).
    /// Returns the id.
    pub fn save_item(
        &mut self,
        vault_id: &str,
        json: &str,
        now_ms: i64,
    ) -> Result<String, ClientError> {
        let vault_id = parse_id(vault_id)?;
        let (item_id, edited) = Item::from_json_with_id(json)?;
        let engine = self.engine()?;
        let item = match engine.item(&vault_id, &item_id)? {
            Some(mut current) => {
                current.apply_edit(edited, now_ms)?;
                current
            }
            None => edited,
        };
        engine.save_item(&vault_id, &item_id, &item)?;
        Ok(item_id.to_string())
    }

    pub fn delete_item(&mut self, vault_id: &str, item_id: &str) -> Result<(), ClientError> {
        let (vault_id, item_id) = (parse_id(vault_id)?, parse_item_id(item_id)?);
        Ok(self.engine()?.delete_item(&vault_id, &item_id)?)
    }

    /// Moves an item to the trash (`restore` false) or back out (`true`).
    pub fn set_trashed(
        &mut self,
        vault_id: &str,
        item_id: &str,
        trashed: bool,
        now_ms: i64,
    ) -> Result<(), ClientError> {
        let (vault_id, item_id) = (parse_id(vault_id)?, parse_item_id(item_id)?);
        let engine = self.engine()?;
        let mut item = engine
            .item(&vault_id, &item_id)?
            .ok_or(ClientError::UnknownItem)?;
        if trashed {
            item.trash(now_ms)?;
        } else {
            item.restore_from_trash(now_ms)?;
        }
        Ok(engine.save_item(&vault_id, &item_id, &item)?)
    }

    /// Deletes items in the trash for 30 days; call at unlock (D19).
    pub fn purge_trash(&mut self, now_ms: i64) -> Result<usize, ClientError> {
        Ok(self.engine()?.purge_trash(now_ms)?)
    }
}
