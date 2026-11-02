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

fn step(step: pwm_client::Step) -> Step {
    match step {
        pwm_client::Step::Send(request) => Step::Send {
            request: Request {
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
            },
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
    ) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_login(
            &email,
            &password,
            &secret_key,
            &device_id,
            platform.into(),
        )?))
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

    pub fn account_id(&self) -> Result<Option<String>, PwmError> {
        Ok(self.lock_inner().account_id()?)
    }

    pub fn start_sync(&self) -> Result<Step, PwmError> {
        Ok(step(self.lock_inner().start_sync()?))
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
        let response = pwm_client::HttpResponse::new(
            response.status,
            response
                .headers
                .into_iter()
                .map(|h| (h.name, h.value))
                .collect(),
            response.body,
        );
        Ok(step(self.lock_inner().receive(response, now_ms)?))
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
