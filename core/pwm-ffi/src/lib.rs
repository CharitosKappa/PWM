//! UniFFI bindings (Swift, Kotlin) over `pwm-client` (decision D31). This
//! crate only translates types and errors; all logic and its tests live in
//! `pwm-client`.
//!
//! This is the only core crate that may contain `unsafe` code, and only in
//! the glue UniFFI generates; nothing here is written by hand.

use std::sync::{Arc, Mutex, MutexGuard};

use pwm_client::{Client, ClientError, SyncEvent, Tokens};
use pwm_store::SqliteStore;

uniffi::setup_scaffolding!();

/// Every error, as the stable code from `ClientError::code` (for example
/// `wrong_password`). Never carries values.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PwmError {
    #[error("{code}")]
    Failed { code: String },
}

impl From<ClientError> for PwmError {
    fn from(err: ClientError) -> Self {
        Self::Failed {
            code: err.code().to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Platform {
    Ios,
    Android,
    Macos,
    Windows,
    Linux,
    Extension,
}

impl From<Platform> for pwm_client::Platform {
    fn from(p: Platform) -> Self {
        match p {
            Platform::Ios => Self::Ios,
            Platform::Android => Self::Android,
            Platform::Macos => Self::Macos,
            Platform::Windows => Self::Windows,
            Platform::Linux => Self::Linux,
            Platform::Extension => Self::Extension,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A request to send with the platform's HTTP stack, relative to the
/// server's base URL. It may carry a token: never log it.
#[derive(Clone, uniffi::Record)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<Header>,
    pub body: Option<Vec<u8>>,
}

#[derive(Clone, uniffi::Record)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

#[derive(uniffi::Enum)]
pub enum Outcome {
    /// Show the Secret Key on the Emergency Kit now; it is not shown again.
    SignedUp {
        secret_key: String,
    },
    LoggedIn,
    VaultCreated {
        vault_id: String,
    },
    Synced {
        pulled: u64,
        pushed: u64,
        conflicts: u64,
        failed: u64,
        resynced: bool,
    },
    SignedOut,
    DeviceRevoked,
    /// The account's devices (D43 §6).
    Devices {
        devices: Vec<Device>,
    },
    /// A device was renamed or revoked.
    DeviceUpdated,
    /// The account's open device approval requests (D46).
    Approvals {
        approvals: Vec<ApprovalRequest>,
    },
    ApprovalUpdated,
    /// This device's push token was registered or removed (D50 §3).
    PushTokenSet,
}

/// A request from a new device; `code` once this device accepted it and the
/// new device revealed its key.
#[derive(uniffi::Record)]
pub struct ApprovalRequest {
    pub id: String,
    pub platform: String,
    pub created_at: i64,
    pub state: String,
    pub code: Option<String>,
}

/// One of the account's devices; `name` is `None` when it has none or it
/// does not open. Times are Unix ms.
#[derive(uniffi::Record)]
pub struct Device {
    pub id: String,
    pub platform: String,
    pub name: Option<String>,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub revoked_at: Option<i64>,
    pub current: bool,
}

#[derive(uniffi::Enum)]
pub enum Step {
    Send { request: Request },
    Wait { delay_ms: u64 },
    Done { outcome: Outcome },
}

#[derive(uniffi::Enum)]
pub enum Event {
    /// Store these in secure storage, replacing the old ones.
    Tokens { access: String, refresh: String },
    /// The server refused this item ("couldn't sync"); it stays queued.
    ItemFailed {
        vault_id: String,
        item_id: String,
        status: u16,
    },
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn request_of(request: &pwm_client::HttpRequest) -> Request {
    Request {
        method: request.method.as_str().to_owned(),
        path: request.path.clone(),
        headers: request
            .headers
            .iter()
            .map(|(n, v)| Header {
                name: (*n).to_owned(),
                value: v.clone(),
            })
            .collect(),
        body: request.body.clone(),
    }
}

fn response_of(response: Response) -> pwm_client::HttpResponse {
    pwm_client::HttpResponse::new(
        response.status,
        response
            .headers
            .into_iter()
            .map(|h| (h.name, h.value))
            .collect(),
        response.body,
    )
}

fn step(step: pwm_client::Step) -> Step {
    match step {
        pwm_client::Step::Send(request) => Step::Send {
            request: request_of(&request),
        },
        pwm_client::Step::Wait { delay_ms } => Step::Wait { delay_ms },
        pwm_client::Step::Done(outcome) => Step::Done {
            outcome: match outcome {
                pwm_client::Outcome::SignedUp { secret_key } => Outcome::SignedUp {
                    secret_key: secret_key.expose_secret().clone(),
                },
                pwm_client::Outcome::LoggedIn => Outcome::LoggedIn,
                pwm_client::Outcome::VaultCreated { vault_id } => {
                    Outcome::VaultCreated { vault_id }
                }
                pwm_client::Outcome::Synced(r) => Outcome::Synced {
                    pulled: count(r.pulled),
                    pushed: count(r.pushed),
                    conflicts: count(r.conflicts),
                    failed: count(r.failed),
                    resynced: r.resynced,
                },
                pwm_client::Outcome::SignedOut => Outcome::SignedOut,
                pwm_client::Outcome::DeviceRevoked => Outcome::DeviceRevoked,
                pwm_client::Outcome::Devices(devices) => Outcome::Devices {
                    devices: devices
                        .into_iter()
                        .map(|d| Device {
                            id: d.id,
                            platform: d.platform,
                            name: d.name,
                            created_at: d.created_at,
                            last_seen_at: d.last_seen_at,
                            revoked_at: d.revoked_at,
                            current: d.current,
                        })
                        .collect(),
                },
                pwm_client::Outcome::DeviceUpdated => Outcome::DeviceUpdated,
                pwm_client::Outcome::Approvals(approvals) => Outcome::Approvals {
                    approvals: approvals
                        .into_iter()
                        .map(|a| ApprovalRequest {
                            id: a.id,
                            platform: a.platform,
                            created_at: a.created_at,
                            state: a.state,
                            code: a.code,
                        })
                        .collect(),
                },
                pwm_client::Outcome::ApprovalUpdated => Outcome::ApprovalUpdated,
                pwm_client::Outcome::PushTokenSet => Outcome::PushTokenSet,
            },
        },
    }
}

/// A login the page may get (D33), best first. `decision` is `fill`,
/// `warn_insecure`, `warn_regex`, `refuse_cross_site`, `refuse_blocked`
/// or `confirm`; every fill still waits for the user.
#[derive(uniffi::Record)]
pub struct AutofillMatch {
    pub vault_id: String,
    pub item_id: String,
    pub title: String,
    pub username: String,
    pub matched: String,
    pub decision: String,
}

#[derive(uniffi::Record)]
pub struct CurrentCode {
    pub code: String,
    pub seconds_left: u64,
}

/// A saved login an update could go to (D36).
#[derive(uniffi::Record)]
pub struct SaveCandidate {
    pub vault_id: String,
    pub item_id: String,
    pub title: String,
    pub username: String,
}

/// What to offer after a sign-in (D36): nothing, saving a new login, or
/// updating the password of these logins.
#[derive(uniffi::Enum)]
pub enum SaveOffer {
    Nothing,
    Save,
    Update { candidates: Vec<SaveCandidate> },
}

/// What a passkey `create` gets (D37).
#[derive(uniffi::Enum)]
pub enum PasskeyCreateCheck {
    Ready { rp_id: String, user_name: String },
    Excluded,
    Unsupported,
}

/// A stored passkey a sign-in may use (never its key).
#[derive(uniffi::Record)]
pub struct PasskeyCandidate {
    pub vault_id: String,
    pub item_id: String,
    pub credential_id: String,
    pub user_name: String,
    pub title: String,
}

/// A passkey made from a platform request that gave only the client data
/// hash (D47).
#[derive(uniffi::Record)]
pub struct HashedPasskey {
    pub credential_id: Vec<u8>,
    pub attestation_object: Vec<u8>,
}

/// What the platform puts in its passkey assertion (D47).
#[derive(uniffi::Record)]
pub struct HashedAssertion {
    pub credential_id: Vec<u8>,
    pub user_handle: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub signature: Vec<u8>,
}

/// A suggestion the system may show above the keyboard before unlock
/// (D47): a domain or RP id and a username, never a password.
#[derive(uniffi::Record)]
pub struct CredentialIdentity {
    pub vault_id: String,
    pub item_id: String,
    /// `password` or `passkey`.
    pub kind: String,
    pub service: String,
    pub user: String,
    pub credential_id: Vec<u8>,
    pub user_handle: Vec<u8>,
}

/// The item and step (`password` or `totp`) to offer next on a tab.
#[derive(uniffi::Record)]
pub struct AutofillNextStep {
    pub item_id: String,
    pub step: String,
}

/// The app's handle on the core: one per signed-in device.
#[derive(uniffi::Object)]
pub struct PwmClient {
    inner: Mutex<Client<SqliteStore>>,
}

fn invalid() -> PwmError {
    ClientError::InvalidItem.into()
}

#[uniffi::export]
impl PwmClient {
    /// Opens (or creates) the cache at `path`, in the app's private storage.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, PwmError> {
        let store = SqliteStore::open(path).map_err(|_| PwmError::from(ClientError::Storage))?;
        Ok(Arc::new(Self {
            inner: Mutex::new(Client::new(store)),
        }))
    }

    /// A cache in memory only (tests, previews).
    #[uniffi::constructor]
    pub fn in_memory() -> Result<Arc<Self>, PwmError> {
        let store =
            SqliteStore::open_in_memory().map_err(|_| PwmError::from(ClientError::Storage))?;
        Ok(Arc::new(Self {
            inner: Mutex::new(Client::new(store)),
        }))
    }

    pub fn start_signup(
        &self,
        email: String,
        password: String,
        device_id: String,
        platform: Platform,
        device_name: String,
    ) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_signup(
            &email,
            &password,
            &device_id,
            platform.into(),
            &device_name,
        )?))
    }

    pub fn start_login(
        &self,
        email: String,
        password: String,
        secret_key: String,
        device_id: String,
        platform: Platform,
        device_name: String,
    ) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_login(
            &email,
            &password,
            &secret_key,
            &device_id,
            platform.into(),
            &device_name,
        )?))
    }

    /// The opened keyset for Face ID / Touch ID on this device (D45): keep
    /// `blob`, and `key` only wrapped by a Secure Enclave key. Never log either.
    pub fn export_biometric_unlock(&self, device_id: String) -> Result<BiometricUnlock, PwmError> {
        let unlock = self.lock_inner().export_biometric_unlock(&device_id)?;
        Ok(BiometricUnlock {
            blob: unlock.blob,
            key: unlock.key.expose_secret().to_vec(),
        })
    }

    /// Unlocks with the blob and the unwrapped key; `biometric_unlock_failed`
    /// means the master password is needed.
    pub fn unlock_biometric(
        &self,
        blob: Vec<u8>,
        key: Vec<u8>,
        device_id: String,
        access: String,
        refresh: String,
    ) -> Result<(), PwmError> {
        Ok(self.lock_inner().unlock_biometric(
            &blob,
            &key,
            &device_id,
            Tokens::new(access, refresh),
        )?)
    }

    /// The account's devices; ends with `Devices`.
    pub fn start_list_devices(&self) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_list_devices()?))
    }

    /// Renames one of the account's devices; ends with `DeviceUpdated`.
    pub fn start_rename_device(&self, device_id: String, name: String) -> Result<Step, PwmError> {
        Ok(step(
            self.lock_inner().start_rename_device(&device_id, &name)?,
        ))
    }

    /// Revokes another device; ends with `DeviceUpdated`.
    pub fn start_revoke_device(&self, device_id: String) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_revoke_device(&device_id)?))
    }

    /// Weak and reused passwords across every vault (D22).
    pub fn health_report(&self) -> Result<HealthReport, PwmError> {
        let report = self.lock_inner().health_report()?;
        Ok(HealthReport {
            weak: report
                .weak
                .into_iter()
                .map(|w| WeakItem {
                    item_id: w.item_id,
                    score: w.score,
                })
                .collect(),
            reused: report.reused,
        })
    }

    /// A breach check over the logins as they are now; the platform fetches
    /// each prefix from `breach_range_url`.
    pub fn breach_check(&self) -> Result<Arc<BreachCheck>, PwmError> {
        let check = self.lock_inner().breach_check()?;
        Ok(Arc::new(BreachCheck {
            inner: Mutex::new(Some(check)),
        }))
    }

    /// The master password again, before an export (D24). The grant stays in
    /// the core for the next `export`, for 5 minutes.
    pub fn authorize_export(
        &self,
        password: String,
        secret_key: String,
        now_ms: i64,
    ) -> Result<(), PwmError> {
        Ok(self
            .lock_inner()
            .authorize_export(&password, &secret_key, now_ms)?)
    }

    /// Reads a picked file to import into `vault_id` (default: the first
    /// vault) and says what will come in (D51). Nothing is saved yet. Never
    /// log the bytes.
    // Each input is one thing the import screen collects.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_import(
        &self,
        vault_id: Option<String>,
        format: String,
        bytes: Vec<u8>,
        password: Option<String>,
        key_file: Option<Vec<u8>>,
        mapping: Option<ImportMapping>,
        now_ms: i64,
    ) -> Result<ImportPreview, PwmError> {
        let bytes = zeroize::Zeroizing::new(bytes);
        let key_file = key_file.map(zeroize::Zeroizing::new);
        Ok(self
            .lock_inner()
            .prepare_import(
                vault_id.as_deref(),
                &format,
                &bytes,
                password.as_deref(),
                key_file.as_deref().map(Vec::as_slice),
                mapping.map(Into::into),
                now_ms,
            )?
            .into())
    }

    /// Saves the prepared import; sync afterwards.
    pub fn finish_import(&self, now_ms: i64) -> Result<ImportPreview, PwmError> {
        Ok(self.lock_inner().finish_import(now_ms)?.into())
    }

    pub fn cancel_import(&self) {
        self.lock_inner().cancel_import();
    }

    /// Every vault as a file, using up the grant. Never log the bytes.
    pub fn export(&self, format: ExportFormat, now_ms: i64) -> Result<Vec<u8>, PwmError> {
        let mut client = self.lock_inner();
        let file = match &format {
            ExportFormat::Pwmx { password } => {
                client.export(pwm_client::ExportFormat::Pwmx(password), now_ms)?
            }
            ExportFormat::Json => client.export(pwm_client::ExportFormat::Json, now_ms)?,
            ExportFormat::Csv => client.export(pwm_client::ExportFormat::Csv, now_ms)?,
        };
        Ok(file.expose_secret().clone())
    }

    pub fn unlock(
        &self,
        password: String,
        secret_key: String,
        access: String,
        refresh: String,
    ) -> Result<(), PwmError> {
        Ok(self
            .lock_inner()
            .unlock(&password, &secret_key, Tokens::new(access, refresh))?)
    }

    pub fn lock(&self) {
        self.lock_inner().lock();
    }

    pub fn is_unlocked(&self) -> bool {
        self.lock_inner().is_unlocked()
    }

    /// Locks and deletes this account's cache; delete the stored tokens and
    /// Secret Key too.
    pub fn sign_out(&self) -> Result<(), PwmError> {
        Ok(self.lock_inner().sign_out()?)
    }

    pub fn account_id(&self) -> Result<Option<String>, PwmError> {
        Ok(self.lock_inner().account_id()?)
    }

    pub fn start_sync(&self) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_sync()?))
    }

    /// Registers this device's APNs token (hex) on the server, or removes it
    /// when `token` is `None`; works locked (D50 §3). `app` is `"ios"` or
    /// `"macos"`. Ends with `PushTokenSet`.
    pub fn start_push_token(
        &self,
        access: String,
        refresh: String,
        app: String,
        token: Option<String>,
        sandbox: bool,
    ) -> Result<Step, PwmError> {
        let push = token.map(|token| pwm_client::PushToken {
            app,
            token,
            sandbox,
        });
        Ok(step(
            self.lock_inner()
                .start_push_token(Tokens::new(access, refresh), push)?,
        ))
    }

    /// A sync that needs only the tokens (D50 §1): locked, it stores what
    /// needs no key and uploads what is queued; unlocked, a normal sync.
    pub fn start_locked_sync(&self, access: String, refresh: String) -> Result<Step, PwmError> {
        Ok(step(
            self.lock_inner()
                .start_locked_sync(Tokens::new(access, refresh))?,
        ))
    }

    /// Sync, then create a vault only if the account has none.
    pub fn start_ensure_vault(&self) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_ensure_vault()?))
    }

    /// The current one-time code of a login, if it has one.
    pub fn current_code(
        &self,
        vault_id: String,
        item_id: String,
        now_ms: i64,
    ) -> Result<Option<CurrentCode>, PwmError> {
        Ok(self
            .lock_inner()
            .current_code(&vault_id, &item_id, now_ms)?
            .map(|(code, left)| CurrentCode {
                code: code.expose_secret().clone(),
                seconds_left: left,
            }))
    }

    pub fn start_create_vault(&self) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_create_vault()?))
    }

    pub fn receive(&self, response: Response, now_ms: i64) -> Result<Step, PwmError> {
        Ok(step(
            self.lock_inner().receive(response_of(response), now_ms)?,
        ))
    }

    /// The account's open device approval requests (D46); ends with `Approvals`.
    pub fn start_list_approvals(&self) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_list_approvals()?))
    }

    /// Accepts a listed request; list again for the code.
    pub fn start_accept_approval(&self, approval_id: String) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_accept_approval(&approval_id)?))
    }

    /// After the user saw the same code on both devices: sends this device's
    /// Secret Key, sealed for the new one.
    pub fn start_complete_approval(
        &self,
        approval_id: String,
        secret_key: String,
    ) -> Result<Step, PwmError> {
        Ok(step(
            self.lock_inner()
                .start_complete_approval(&approval_id, &secret_key)?,
        ))
    }

    pub fn start_deny_approval(&self, approval_id: String) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_deny_approval(&approval_id)?))
    }

    pub fn network_failed(&self) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().network_failed()?))
    }

    pub fn cancel(&self) {
        self.lock_inner().cancel();
    }

    pub fn take_events(&self) -> Vec<Event> {
        self.lock_inner()
            .take_events()
            .into_iter()
            .map(|e| match e {
                SyncEvent::TokensRefreshed(t) => Event::Tokens {
                    access: t.access().to_owned(),
                    refresh: t.refresh().to_owned(),
                },
                SyncEvent::ItemFailed {
                    vault_id,
                    item_id,
                    status,
                } => Event::ItemFailed {
                    vault_id: pwm_client::id_text(&vault_id),
                    item_id: item_id.to_string(),
                    status,
                },
            })
            .collect()
    }

    pub fn vault_ids(&self) -> Result<Vec<String>, PwmError> {
        Ok(self.lock_inner().vault_ids()?)
    }

    /// Items as D18 JSON with their id.
    pub fn items(&self, vault_id: String) -> Result<Vec<String>, PwmError> {
        Ok(self
            .lock_inner()
            .items(&vault_id)?
            .into_iter()
            .map(|json| json.expose_secret().clone())
            .collect())
    }

    pub fn item(&self, vault_id: String, item_id: String) -> Result<Option<String>, PwmError> {
        Ok(self
            .lock_inner()
            .item(&vault_id, &item_id)?
            .map(|json| json.expose_secret().clone()))
    }

    pub fn save_item(
        &self,
        vault_id: String,
        json: String,
        now_ms: i64,
    ) -> Result<String, PwmError> {
        Ok(self.lock_inner().save_item(&vault_id, &json, now_ms)?)
    }

    pub fn delete_item(&self, vault_id: String, item_id: String) -> Result<(), PwmError> {
        Ok(self.lock_inner().delete_item(&vault_id, &item_id)?)
    }

    pub fn set_trashed(
        &self,
        vault_id: String,
        item_id: String,
        trashed: bool,
        now_ms: i64,
    ) -> Result<(), PwmError> {
        Ok(self
            .lock_inner()
            .set_trashed(&vault_id, &item_id, trashed, now_ms)?)
    }

    pub fn purge_trash(&self, now_ms: i64) -> Result<u64, PwmError> {
        Ok(count(self.lock_inner().purge_trash(now_ms)?))
    }

    /// Checks a passkey `create` before asking the user (D37).
    pub fn passkey_create_check(
        &self,
        origin: String,
        top_origin: Option<String>,
        request_json: String,
    ) -> Result<PasskeyCreateCheck, PwmError> {
        Ok(
            match self.lock_inner().passkey_create_check(
                &origin,
                top_origin.as_deref(),
                &request_json,
            )? {
                pwm_client::PasskeyCreateCheck::Ready { rp_id, user_name } => {
                    PasskeyCreateCheck::Ready { rp_id, user_name }
                }
                pwm_client::PasskeyCreateCheck::Excluded => PasskeyCreateCheck::Excluded,
                pwm_client::PasskeyCreateCheck::Unsupported => PasskeyCreateCheck::Unsupported,
            },
        )
    }

    /// Creates and stores a passkey; returns `RegistrationResponseJSON`.
    pub fn passkey_create(
        &self,
        vault_id: String,
        origin: String,
        top_origin: Option<String>,
        request_json: String,
        now_ms: i64,
    ) -> Result<String, PwmError> {
        Ok(self.lock_inner().passkey_create(
            &vault_id,
            &origin,
            top_origin.as_deref(),
            &request_json,
            now_ms,
        )?)
    }

    /// The passkeys a `get` may use.
    pub fn passkey_candidates(
        &self,
        origin: String,
        top_origin: Option<String>,
        request_json: String,
    ) -> Result<Vec<PasskeyCandidate>, PwmError> {
        Ok(self
            .lock_inner()
            .passkey_candidates(&origin, top_origin.as_deref(), &request_json)?
            .into_iter()
            .map(|c| PasskeyCandidate {
                vault_id: c.vault_id,
                item_id: c.item_id,
                credential_id: c.credential_id,
                user_name: c.user_name,
                title: c.title,
            })
            .collect())
    }

    /// Signs a `get`; returns `AuthenticationResponseJSON`.
    pub fn passkey_get(
        &self,
        origin: String,
        top_origin: Option<String>,
        request_json: String,
        vault_id: String,
        item_id: String,
        credential_id: String,
    ) -> Result<String, PwmError> {
        Ok(self.lock_inner().passkey_get(
            &origin,
            top_origin.as_deref(),
            &request_json,
            &vault_id,
            &item_id,
            &credential_id,
        )?)
    }

    /// Creates and stores a passkey for a platform request that gave only
    /// the client data hash (D47).
    pub fn passkey_register_hashed(
        &self,
        vault_id: String,
        rp_id: String,
        user_name: String,
        user_handle: Vec<u8>,
        now_ms: i64,
    ) -> Result<HashedPasskey, PwmError> {
        let made = self.lock_inner().passkey_register_hashed(
            &vault_id,
            &rp_id,
            &user_name,
            &user_handle,
            now_ms,
        )?;
        Ok(HashedPasskey {
            credential_id: made.credential_id,
            attestation_object: made.attestation_object,
        })
    }

    /// Signs a platform sign-in given its client data hash (D47).
    pub fn passkey_assert_hashed(
        &self,
        vault_id: String,
        item_id: String,
        credential_id: Vec<u8>,
        rp_id: String,
        client_data_hash: Vec<u8>,
    ) -> Result<HashedAssertion, PwmError> {
        let signed = self.lock_inner().passkey_assert_hashed(
            &vault_id,
            &item_id,
            &credential_id,
            &rp_id,
            &client_data_hash,
        )?;
        Ok(HashedAssertion {
            credential_id: signed.credential_id,
            user_handle: signed.user_handle,
            authenticator_data: signed.authenticator_data,
            signature: signed.signature,
        })
    }

    /// What the system may suggest above the keyboard (D47).
    pub fn credential_identities(&self) -> Result<Vec<CredentialIdentity>, PwmError> {
        Ok(self
            .lock_inner()
            .credential_identities()?
            .into_iter()
            .map(|i| CredentialIdentity {
                vault_id: i.vault_id,
                item_id: i.item_id,
                kind: i.kind.to_owned(),
                service: i.service,
                user: i.user,
                credential_id: i.credential_id,
                user_handle: i.user_handle,
            })
            .collect())
    }

    /// What to offer after a sign-in on `page_url` (D36).
    pub fn save_offer(
        &self,
        page_url: String,
        frame_url: Option<String>,
        username: String,
        password: String,
        new_password: Option<String>,
    ) -> Result<SaveOffer, PwmError> {
        let offer = self.lock_inner().save_offer(
            &page_url,
            frame_url.as_deref(),
            &username,
            &password,
            new_password.as_deref(),
        )?;
        Ok(match offer {
            pwm_client::SaveOffer::Nothing => SaveOffer::Nothing,
            pwm_client::SaveOffer::Save => SaveOffer::Save,
            pwm_client::SaveOffer::Update(list) => SaveOffer::Update {
                candidates: list
                    .into_iter()
                    .map(|c| SaveCandidate {
                        vault_id: c.vault_id,
                        item_id: c.item_id,
                        title: c.title,
                        username: c.username,
                    })
                    .collect(),
            },
        })
    }

    /// Saves a new login for the site of `page_url`; returns its id.
    pub fn save_login(
        &self,
        vault_id: String,
        page_url: String,
        title: String,
        username: String,
        password: String,
        now_ms: i64,
    ) -> Result<String, PwmError> {
        Ok(self
            .lock_inner()
            .save_login(&vault_id, &page_url, &title, &username, &password, now_ms)?)
    }

    /// Replaces a login's password; the old one goes to history.
    pub fn update_password(
        &self,
        vault_id: String,
        item_id: String,
        password: String,
        now_ms: i64,
    ) -> Result<(), PwmError> {
        Ok(self
            .lock_inner()
            .update_password(&vault_id, &item_id, &password, now_ms)?)
    }

    /// An "Autofill didn't work here" report (D38), as JSON.
    pub fn autofill_report(
        &self,
        page_url: String,
        problem: String,
        frames_json: String,
        app_version: String,
        browser: String,
    ) -> Result<String, PwmError> {
        Ok(self.lock_inner().autofill_report(
            &page_url,
            &problem,
            &frames_json,
            &app_version,
            &browser,
        )?)
    }

    /// The logins this page (or the frame holding the fields) may get.
    pub fn autofill_matches(
        &self,
        page_url: String,
        frame_url: Option<String>,
    ) -> Result<Vec<AutofillMatch>, PwmError> {
        Ok(self
            .lock_inner()
            .autofill_matches(&page_url, frame_url.as_deref())?
            .into_iter()
            .map(|m| AutofillMatch {
                vault_id: m.vault_id,
                item_id: m.item_id,
                title: m.title,
                username: m.username,
                matched: m.matched.to_owned(),
                decision: m.decision.to_owned(),
            })
            .collect())
    }

    /// The logins linked to an Android app and one of its signing
    /// certificates' SHA-256 digests (D58 §4), linked ones first.
    pub fn autofill_app_matches(
        &self,
        package: String,
        cert_sha256: Vec<String>,
    ) -> Result<Vec<AutofillMatch>, PwmError> {
        Ok(self
            .lock_inner()
            .autofill_app_matches(&package, &cert_sha256)?
            .into_iter()
            .map(|m| AutofillMatch {
                vault_id: m.vault_id,
                item_id: m.item_id,
                title: m.title,
                username: m.username,
                matched: m.matched.to_owned(),
                decision: m.decision.to_owned(),
            })
            .collect())
    }

    /// Field descriptions (JSON array, never values) in, `[{class, confidence}]` out.
    pub fn classify_fields(
        &self,
        fields_json: String,
        page_url: String,
    ) -> Result<String, PwmError> {
        Ok(self.lock_inner().classify_fields(&fields_json, &page_url)?)
    }

    /// Accepts a signed rules file; returns its version to store.
    pub fn set_autofill_rules(
        &self,
        bytes: Vec<u8>,
        now_ms: i64,
        min_version: u64,
    ) -> Result<u64, PwmError> {
        Ok(self
            .lock_inner()
            .set_autofill_rules(&bytes, now_ms, min_version)?)
    }

    pub fn autofill_record_step(
        &self,
        tab: String,
        page_url: String,
        item_id: String,
        step: String,
        now_ms: i64,
    ) -> Result<(), PwmError> {
        Ok(self
            .lock_inner()
            .autofill_record_step(&tab, &page_url, &item_id, &step, now_ms)?)
    }

    pub fn autofill_next_step(
        &self,
        tab: String,
        page_url: String,
        now_ms: i64,
    ) -> Option<AutofillNextStep> {
        self.lock_inner()
            .autofill_next_step(&tab, &page_url, now_ms)
            .map(|(item_id, step)| AutofillNextStep {
                item_id,
                step: step.to_owned(),
            })
    }

    pub fn autofill_forget_tab(&self, tab: String) {
        self.lock_inner().autofill_forget_tab(&tab);
    }
}

impl PwmClient {
    /// A panic elsewhere never leaves the client unusable.
    fn lock_inner(&self) -> MutexGuard<'_, Client<SqliteStore>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(uniffi::Record)]
pub struct Generated {
    pub value: String,
    pub entropy_bits: f64,
}

#[derive(uniffi::Record)]
pub struct Strength {
    /// 0 (too guessable) to 4.
    pub score: u8,
    pub guesses_log10: f64,
    pub crack_time: String,
    pub warning: Option<String>,
    pub suggestions: Vec<String>,
}

/// What an import brings in and leaves out (D51).
#[derive(uniffi::Record)]
pub struct ImportPreview {
    pub logins: u64,
    pub notes: u64,
    pub cards: u64,
    pub identities: u64,
    pub folders: u64,
    pub trashed: u64,
    pub left_out: Vec<LeftOutCount>,
}

/// A reason (`attachments`, `ssh_keys`, `aliases`, `passkeys`,
/// `unknown_types`, `empty`, `too_large`, `duplicates`) and how many.
#[derive(uniffi::Record)]
pub struct LeftOutCount {
    pub reason: String,
    pub count: u64,
}

impl From<pwm_client::ImportPreview> for ImportPreview {
    fn from(p: pwm_client::ImportPreview) -> Self {
        let n = |v: usize| v as u64;
        Self {
            logins: n(p.logins),
            notes: n(p.notes),
            cards: n(p.cards),
            identities: n(p.identities),
            folders: n(p.folders),
            trashed: n(p.trashed),
            left_out: p
                .left_out
                .into_iter()
                .map(|(reason, count)| LeftOutCount {
                    reason,
                    count: n(count),
                })
                .collect(),
        }
    }
}

/// Which CSV column holds each field, by index.
#[derive(uniffi::Record)]
pub struct ImportMapping {
    pub title: Option<u32>,
    pub url: Option<u32>,
    pub username: Option<u32>,
    pub password: Option<u32>,
    pub notes: Option<u32>,
    pub totp: Option<u32>,
    pub folder: Option<u32>,
    pub favorite: Option<u32>,
    pub tags: Option<u32>,
}

impl From<ImportMapping> for pwm_client::ColumnMapping {
    fn from(m: ImportMapping) -> Self {
        let i = |v: Option<u32>| v.map(|v| v as usize);
        Self {
            title: i(m.title),
            url: i(m.url),
            username: i(m.username),
            password: i(m.password),
            notes: i(m.notes),
            totp: i(m.totp),
            folder: i(m.folder),
            favorite: i(m.favorite),
            tags: i(m.tags),
        }
    }
}

impl From<pwm_client::ColumnMapping> for ImportMapping {
    fn from(m: pwm_client::ColumnMapping) -> Self {
        let i = |v: Option<usize>| v.and_then(|v| u32::try_from(v).ok());
        Self {
            title: i(m.title),
            url: i(m.url),
            username: i(m.username),
            password: i(m.password),
            notes: i(m.notes),
            totp: i(m.totp),
            folder: i(m.folder),
            favorite: i(m.favorite),
            tags: i(m.tags),
        }
    }
}

#[derive(uniffi::Record)]
pub struct ImportColumns {
    pub headers: Vec<String>,
    pub suggested: ImportMapping,
    pub rows: u64,
}

#[derive(uniffi::Record)]
pub struct DetectedImport {
    /// `bitwarden`, `1password`, `keeper`, `keepass`, `nordpass`, `protonpass`,
    /// `dashlane`, `lastpass`, `csv` or `pwm`.
    pub format: String,
    pub needs_password: bool,
}

/// The format a picked file looks like, or none.
#[uniffi::export]
pub fn detect_import(file_name: String, bytes: Vec<u8>) -> Option<DetectedImport> {
    let bytes = zeroize::Zeroizing::new(bytes);
    pwm_client::detect_import(&file_name, &bytes).map(|(format, needs_password)| DetectedImport {
        format,
        needs_password,
    })
}

/// A CSV's header row and the suggested columns, for the column picker.
#[uniffi::export]
pub fn import_columns(bytes: Vec<u8>) -> Result<ImportColumns, PwmError> {
    let bytes = zeroize::Zeroizing::new(bytes);
    let columns = pwm_client::import_columns(&bytes).map_err(pwm_client::ClientError::from)?;
    Ok(ImportColumns {
        headers: columns.headers.clone(),
        suggested: columns.suggested.clone().into(),
        rows: columns.rows as u64,
    })
}

/// A new random id for an item or a device.
#[uniffi::export]
pub fn new_id() -> Result<String, PwmError> {
    Ok(pwm_client::new_id()?)
}

#[uniffi::export]
pub fn generate_password(
    length: u32,
    lowercase: bool,
    uppercase: bool,
    digits: bool,
    symbols: bool,
    avoid_lookalikes: bool,
) -> Result<Generated, PwmError> {
    let options = pwm_client::PasswordOptions {
        length: usize::try_from(length).map_err(|_| invalid())?,
        lowercase,
        uppercase,
        digits,
        symbols,
        avoid_lookalikes,
    };
    let generated = pwm_client::generate_password(&options).map_err(|_| invalid())?;
    Ok(Generated {
        value: generated.value.expose_secret().clone(),
        entropy_bits: generated.entropy_bits,
    })
}

#[uniffi::export]
pub fn generate_passphrase(
    words: u32,
    separator: String,
    capitalize: bool,
    add_digit: bool,
) -> Result<Generated, PwmError> {
    let options = pwm_client::PassphraseOptions {
        words: usize::try_from(words).map_err(|_| invalid())?,
        separator,
        capitalize,
        add_digit,
    };
    let generated = pwm_client::generate_passphrase(&options).map_err(|_| invalid())?;
    Ok(Generated {
        value: generated.value.expose_secret().clone(),
        entropy_bits: generated.entropy_bits,
    })
}

/// How guessable `password` is; `context` holds words the user is likely to
/// reuse (email, name, item title).
#[uniffi::export]
pub fn password_strength(password: String, context: Vec<String>) -> Strength {
    let context: Vec<&str> = context.iter().map(String::as_str).collect();
    let s = pwm_client::strength(&password, &context);
    Strength {
        score: s.score,
        guesses_log10: s.guesses_log10,
        crack_time: s.crack_time,
        warning: s.warning,
        suggestions: s.suggestions,
    }
}

/// Paper size for the Emergency Kit, from the user's locale.
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Paper {
    A4,
    Letter,
}

impl From<Paper> for pwm_client::Paper {
    fn from(p: Paper) -> Self {
        match p {
            Paper::A4 => Self::A4,
            Paper::Letter => Self::Letter,
        }
    }
}

/// The Emergency Kit as PDF bytes (D42). It holds the Secret Key: hand it
/// to the share sheet or printer and drop it; never write it to a cache.
#[uniffi::export]
pub fn emergency_kit(
    email: String,
    secret_key: String,
    server: Option<String>,
    account_id: String,
    created: String,
    paper: Paper,
) -> Result<Vec<u8>, PwmError> {
    let pdf = pwm_client::emergency_kit(
        &email,
        &secret_key,
        server.as_deref(),
        &account_id,
        &created,
        paper.into(),
    )?;
    Ok(pdf.expose_secret().clone())
}

/// What a scanned Emergency Kit fills in: never a password.
#[derive(uniffi::Record)]
pub struct ScannedKit {
    pub email: String,
    pub secret_key: String,
    /// A self-hosted server's address; `None` for the hosted service.
    pub server: Option<String>,
}

/// Reads the text of a scanned QR code; `not_a_kit` for any other code.
#[uniffi::export]
pub fn scan_kit(text: String) -> Result<ScannedKit, PwmError> {
    let kit = pwm_client::scan_kit(&text)?;
    Ok(ScannedKit {
        email: kit.email,
        secret_key: kit.secret_key.expose_secret().clone(),
        server: kit.server,
    })
}

/// The website-list entry linking a login to an Android app (D58 §4):
/// `android-app://<package>?sha256=<digest>`. `invalid_url` for a bad package
/// name or digest.
#[uniffi::export]
pub fn android_app_link(package: String, cert_sha256: String) -> Result<String, PwmError> {
    pwm_client::app_link(&package, &cert_sha256).map_err(|_| PwmError::Failed {
        code: "invalid_url".to_owned(),
    })
}

/// What to store as a login's one-time code for what the user entered
/// (D23 §3); `invalid_totp` if it is not a code setup.
#[uniffi::export]
pub fn normalize_totp(input: String, title: String) -> Result<String, PwmError> {
    Ok(pwm_client::normalize_totp(&input, &title)?
        .expose_secret()
        .clone())
}

#[derive(uniffi::Record)]
pub struct WeakItem {
    pub item_id: String,
    pub score: u8,
}

#[derive(uniffi::Record)]
pub struct HealthReport {
    pub weak: Vec<WeakItem>,
    /// Groups of logins sharing one password.
    pub reused: Vec<Vec<String>>,
}

#[derive(uniffi::Record)]
pub struct BreachedItem {
    pub item_id: String,
    pub count: u64,
}

/// What to export (D24); `Pwmx` takes the export password.
#[derive(uniffi::Enum)]
pub enum ExportFormat {
    Pwmx { password: String },
    Json,
    Csv,
}

/// One breach check (D22 §4): fetch each prefix, add each body, then finish.
#[derive(uniffi::Object)]
pub struct BreachCheck {
    inner: Mutex<Option<pwm_client::BreachCheck>>,
}

#[uniffi::export]
impl BreachCheck {
    pub fn prefixes(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(pwm_client::BreachCheck::prefixes)
            .unwrap_or_default()
    }

    pub fn add_response(&self, prefix: String, body: String) -> Result<(), PwmError> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let check = inner
            .as_mut()
            .ok_or(PwmError::from(ClientError::NotRunning))?;
        Ok(check.add_response(&prefix, &body)?)
    }

    /// The breached logins; the check is used up.
    pub fn finish(&self) -> Vec<BreachedItem> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .map(pwm_client::BreachCheck::finish)
            .unwrap_or_default()
            .into_iter()
            .map(|b| BreachedItem {
                item_id: b.item_id,
                count: b.count,
            })
            .collect()
    }
}

/// The HIBP range URL for a prefix, sent with `breach_padding_header` and
/// nothing that identifies the account (D22 §4).
#[uniffi::export]
pub fn breach_range_url(prefix: String) -> String {
    format!("{}{prefix}", pwm_client::HIBP_RANGE_URL)
}

/// The header every range request sends, so sizes do not hint at prefixes.
#[uniffi::export]
pub fn breach_padding_header() -> Header {
    Header {
        name: pwm_client::ADD_PADDING_HEADER.0.to_owned(),
        value: pwm_client::ADD_PADDING_HEADER.1.to_owned(),
    }
}

/// What biometric unlock keeps (D45): the blob, and the key to wrap.
#[derive(uniffi::Record)]
pub struct BiometricUnlock {
    pub blob: Vec<u8>,
    pub key: Vec<u8>,
}

/// What the new device does next (D46).
#[derive(uniffi::Enum)]
pub enum AskStep {
    Send {
        request: Request,
    },
    Wait {
        delay_ms: u64,
    },
    /// Show it; the user compares it on the signed-in device. Keep polling.
    ShowCode {
        code: String,
    },
    /// Log in with it and the master password. Never log it.
    Approved {
        secret_key: String,
    },
    Ended,
}

/// The new device's side of an approval: send `first_request`, then
/// `receive` each answer and `poll` when told to wait.
#[derive(uniffi::Object)]
pub struct ApprovalAsk {
    inner: Mutex<pwm_client::ApprovalAsk>,
    first: Mutex<Option<Request>>,
}

#[uniffi::export]
impl ApprovalAsk {
    #[uniffi::constructor]
    pub fn new(email: String, platform: Platform) -> Result<Arc<Self>, PwmError> {
        let (ask, request) = pwm_client::ApprovalAsk::start(&email, platform.into())?;
        Ok(Arc::new(Self {
            inner: Mutex::new(ask),
            first: Mutex::new(Some(request_of(&request))),
        }))
    }

    /// The request that makes it; given once.
    pub fn first_request(&self) -> Option<Request> {
        self.first
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    pub fn poll(&self) -> Result<Request, PwmError> {
        let mut ask = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(request_of(&ask.poll()?))
    }

    /// Gives up; send it without waiting for the answer.
    pub fn cancel(&self) -> Option<Request> {
        let ask = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ask.cancel().as_ref().map(request_of)
    }

    pub fn receive(&self, response: Response) -> Result<AskStep, PwmError> {
        let mut ask = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(match ask.receive(&response_of(response))? {
            pwm_client::AskStep::Send(request) => AskStep::Send {
                request: request_of(&request),
            },
            pwm_client::AskStep::Wait { delay_ms } => AskStep::Wait { delay_ms },
            pwm_client::AskStep::ShowCode { code } => AskStep::ShowCode { code },
            pwm_client::AskStep::Approved { secret_key } => AskStep::Approved {
                secret_key: secret_key.expose_secret().clone(),
            },
            pwm_client::AskStep::Ended => AskStep::Ended,
        })
    }
}
