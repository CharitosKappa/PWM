//! The backend (D66 §3): owns the core, the cache, the device's keys and
//! every server conversation, and answers the screens' requests as the
//! extension's background does (D34). The system's pieces (HTTP, the key
//! store, the clipboard, the clocks) come in as traits, so this is tested
//! with fakes.

use std::time::Duration;

use base64::Engine;
use pwm_client::{
    ApprovalAsk, AskStep, Client, ClientError, HttpRequest, Outcome, PassphraseOptions,
    PasswordOptions, Platform, Secret, Step, SyncEvent, Tokens,
};
use pwm_store::SqliteStore;
use serde_json::{Value, json};

use crate::clipboard::{CLEAR_AFTER_MS, Clipboard};
use crate::http::{Transport, base_url};
use crate::keys::{KeyStore, name};
use crate::lock::AutoLock;
use crate::protocol::{
    AskState, AskView, CopyField, GenerateMode, ItemSummary, Request, Settings, Status,
};
use crate::settings::{self, HOSTED_SERVER, SettingsFile};

/// How long signing out waits for the server (D62).
const SIGN_OUT_WAIT: Duration = Duration::from_secs(5);
/// How long giving up on an approval waits for the server.
const CANCEL_WAIT: Duration = Duration::from_secs(3);

/// A failed request: one of the screens' error codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub String);

impl Failure {
    pub fn code(code: &str) -> Self {
        Self(code.to_owned())
    }
}

impl From<ClientError> for Failure {
    fn from(e: ClientError) -> Self {
        Self::code(e.code())
    }
}

impl From<crate::keys::KeyError> for Failure {
    fn from(_: crate::keys::KeyError) -> Self {
        Self::code("storage")
    }
}

impl From<crate::clipboard::ClipboardError> for Failure {
    fn from(_: crate::clipboard::ClipboardError) -> Self {
        Self::code("clipboard")
    }
}

type Result<T> = std::result::Result<T, Failure>;

/// What the app does after a reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// Sync in the background (after unlocking).
    Sync,
}

/// What the timers asked for, from [`Backend::tick`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    /// The vault locked: show Unlock.
    pub locked: bool,
    /// A sync that failed is due again.
    pub sync: bool,
}

/// The clocks: wall time for the core, monotonic time for the timers.
pub trait Clock: Send + Sync + 'static {
    fn now_ms(&self) -> i64;
    fn mono_ms(&self) -> u64;
}

pub struct SystemClock {
    start: std::time::Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }

    fn mono_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Asking a signed-in device to approve this one (D46), while signed out.
struct Asking {
    ask: ApprovalAsk,
    email: String,
    code: Option<String>,
    /// The Secret Key from the signed-in device, until the password is typed.
    approved: Option<Secret<String>>,
}

/// This computer, as the device list names it (sealed, D43 §6).
pub struct Device {
    pub platform: Platform,
    pub name: String,
}

impl Device {
    pub fn this_computer() -> Self {
        #[cfg(windows)]
        let (platform, fallback, name) = (
            Platform::Windows,
            "Windows computer",
            std::env::var("COMPUTERNAME").ok(),
        );
        #[cfg(not(windows))]
        let (platform, fallback, name) = (
            Platform::Linux,
            "Linux computer",
            std::fs::read_to_string("/etc/hostname").ok(),
        );
        let name = name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty() && n.chars().count() <= 63)
            .unwrap_or_else(|| fallback.to_owned());
        Self { platform, name }
    }
}

/// The login fields the list and copying read from an item's D18 JSON.
struct Login {
    username: String,
    password: Secret<String>,
    totp: bool,
    url: String,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or("").to_owned()
}

fn login_of(item: &Value) -> Login {
    let login = &item["login"];
    Login {
        username: text(&login["username"]),
        password: Secret::new(text(&login["password"])),
        totp: login["totp"].as_str().is_some_and(|t| !t.is_empty()),
        url: text(&login["urls"][0]["url"]),
    }
}

/// `YYYY-MM-DD` in this computer's time zone, for the Emergency Kit.
fn local_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// US Letter in the regions that print on it, A4 elsewhere (as the extension).
fn paper() -> pwm_client::Paper {
    const LETTER: [&str; 15] = [
        "US", "CA", "MX", "PH", "CL", "CO", "VE", "GT", "CR", "PA", "DO", "SV", "NI", "PR", "BZ",
    ];
    let region = sys_locale::get_locale()
        .and_then(|l| l.split(['-', '_']).nth(1).map(|r| r.to_ascii_uppercase()))
        .unwrap_or_default();
    if LETTER.contains(&region.as_str()) {
        pwm_client::Paper::Letter
    } else {
        pwm_client::Paper::A4
    }
}

pub struct Backend<T: Transport, K: KeyStore, C: Clipboard, Q: Clock> {
    client: Client<SqliteStore>,
    transport: T,
    keys: K,
    clipboard: C,
    clock: Q,
    device: Device,
    settings: Settings,
    settings_file: Option<SettingsFile>,
    synced_at: Option<i64>,
    sync_error: Option<String>,
    asking: Option<Asking>,
    auto_lock: AutoLock,
    clear_clipboard_at: Option<u64>,
    retry_sync_at: Option<u64>,
}

impl<T: Transport, K: KeyStore, C: Clipboard, Q: Clock> Backend<T, K, C, Q> {
    /// A locked backend over the cache in `store`; `settings_file` is `None`
    /// in tests.
    pub fn new(
        store: SqliteStore,
        transport: T,
        keys: K,
        clipboard: C,
        clock: Q,
        device: Device,
        settings_file: Option<SettingsFile>,
    ) -> Self {
        let settings = settings_file
            .as_ref()
            .map(SettingsFile::load)
            .unwrap_or_default();
        Self {
            client: Client::new(store),
            transport,
            keys,
            clipboard,
            clock,
            device,
            settings,
            settings_file,
            synced_at: None,
            sync_error: None,
            asking: None,
            auto_lock: AutoLock::default(),
            clear_clipboard_at: None,
            retry_sync_at: None,
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.client.is_unlocked()
    }

    /// Answers one request; every failure is one of the screens' codes.
    pub async fn handle(&mut self, request: Request) -> (Result<Value>, After) {
        let mut after = After::Nothing;
        let result = self.dispatch(request, &mut after).await;
        // Any use restarts the lock timeout.
        self.touch();
        (result, after)
    }

    async fn dispatch(&mut self, request: Request, after: &mut After) -> Result<Value> {
        let null = || Ok(Value::Null);
        match request {
            Request::Status => Ok(json!(self.status().await?)),
            Request::SignUp { email, password } => {
                let secret_key = self.sign_up(&email, &password).await?;
                Ok(json!({ "secretKey": secret_key.expose_secret() }))
            }
            Request::SignIn {
                email,
                password,
                secret_key,
            } => {
                self.sign_in(&email, &password, &secret_key).await?;
                null()
            }
            Request::Unlock { password } => {
                self.unlock(&password).await?;
                *after = After::Sync;
                null()
            }
            Request::SignInAgain { password } => {
                self.sign_in_again(&password).await?;
                Ok(json!(self.status().await?))
            }
            Request::Lock => {
                self.lock();
                null()
            }
            Request::SignOut => {
                self.sign_out().await?;
                null()
            }
            Request::Items { query } => Ok(json!(self.items(&query)?)),
            Request::Copy {
                vault_id,
                item_id,
                field,
            } => {
                self.copy(&vault_id, &item_id, field)?;
                null()
            }
            Request::CopySecretKey => {
                let secret_key = self
                    .keys
                    .get(name::SECRET_KEY)
                    .await?
                    .ok_or_else(|| Failure::code("signed_out"))?;
                self.write_clipboard(secret_key.expose_secret())?;
                null()
            }
            Request::EmergencyKit => {
                let pdf = self.emergency_kit().await?;
                let pdf = base64::engine::general_purpose::STANDARD.encode(pdf.expose_secret());
                Ok(json!({ "pdf": pdf }))
            }
            Request::CopyText { text } => {
                self.write_clipboard(text.expose_secret())?;
                null()
            }
            Request::Generate { mode } => Ok(json!(generate(mode)?.expose_secret())),
            Request::Score { password, email } => Ok(json!(
                pwm_client::strength(password.expose_secret(), &[email.as_str()]).score
            )),
            Request::Sync => Ok(json!(self.sync().await?)),
            Request::Settings => Ok(json!(self.settings)),
            Request::SetSettings { settings } => Ok(json!(self.set_settings(&settings)?)),
            Request::Activity => null(),
            Request::AskApproval { email } => Ok(json!(self.ask(&email).await?)),
            Request::AskApprovalPoll => Ok(json!(self.poll().await?)),
            Request::AskApprovalCancel => {
                self.cancel_ask().await;
                null()
            }
            Request::SignInApproved { password } => {
                let (email, secret_key) = match &mut self.asking {
                    Some(Asking {
                        email,
                        approved: Some(key),
                        ..
                    }) => (email.clone(), Secret::new(key.expose_secret().clone())),
                    _ => return Err(Failure::code("approval_ended")),
                };
                self.sign_in(&email, &password, &secret_key).await?;
                self.asking = None;
                null()
            }
            Request::Unsupported => Err(Failure::code("unsupported")),
        }
    }

    // --- State ---

    pub async fn status(&mut self) -> Result<Status> {
        if self.client.account_id()?.is_none() {
            return Ok(Status::SignedOut);
        }
        let email = self
            .keys
            .get(name::EMAIL)
            .await?
            .map(|e| e.expose_secret().clone())
            .unwrap_or_default();
        if !self.client.is_unlocked() {
            return Ok(Status::Locked {
                email,
                touch_id: false,
            });
        }
        Ok(Status::Unlocked {
            email,
            synced_at: self.synced_at,
            sync_error: self.sync_error.clone(),
            // Read by the manage page's approvals, which come in M7.6.
            approvals: 0,
        })
    }

    /// Restarts the lock timeout (D66 §6): a use, or the window losing focus.
    pub fn touch(&mut self) {
        if self.client.is_unlocked() {
            self.auto_lock
                .touch(self.clock.mono_ms(), self.settings.auto_lock_minutes);
        }
    }

    /// Locks now: sleep, the session locking, the tray's Lock, or the timeout.
    pub fn lock(&mut self) {
        self.client.lock();
        self.auto_lock.clear();
        self.retry_sync_at = None;
    }

    /// Runs the timers; the app calls it about once a second.
    pub fn tick(&mut self) -> Due {
        let now = self.clock.mono_ms();
        let mut due = Due::default();
        if self.clear_clipboard_at.is_some_and(|at| now >= at) {
            self.clear_clipboard_at = None;
            let _ = self.clipboard.clear();
        }
        if self.client.is_unlocked() && self.auto_lock.due(now) {
            self.lock();
            due.locked = true;
        }
        if self.retry_sync_at.is_some_and(|at| now >= at) {
            self.retry_sync_at = None;
            due.sync = self.client.is_unlocked();
        }
        due
    }

    // --- Server conversations ---

    /// Runs `first` and every step after it, then keeps any new tokens.
    async fn run(&mut self, first: std::result::Result<Step, ClientError>) -> Result<Step> {
        let mut step = first?;
        let base = base_url(&self.settings.server_url);
        while let Step::Send(request) = step {
            step = match self.transport.send(&base, request).await {
                Ok(response) => self.client.receive(response, self.clock.now_ms()),
                Err(_) => self.client.network_failed(),
            }
            .inspect_err(|_| self.client.cancel())?;
        }
        self.save_tokens().await?;
        Ok(step)
    }

    /// Keeps the tokens the core handed out: as they are in a secure store,
    /// sealed under `sym_key` otherwise (D34, D66 §4).
    async fn save_tokens(&mut self) -> Result<()> {
        let tokens = self
            .client
            .take_events()
            .into_iter()
            .rev()
            .find_map(|e| match e {
                SyncEvent::TokensRefreshed(t) => Some(t),
                SyncEvent::ItemFailed { .. } => None,
            });
        let Some(tokens) = tokens else {
            return Ok(());
        };
        if self.keys.secure() {
            let pair = Secret::new(
                json!({ "access": tokens.access(), "refresh": tokens.refresh() }).to_string(),
            );
            self.keys.set(name::TOKENS, pair.expose_secret()).await?;
        } else if self.client.is_unlocked() {
            let sealed = self.client.seal_tokens(tokens.access(), tokens.refresh())?;
            let sealed = base64::engine::general_purpose::STANDARD.encode(sealed);
            self.keys.set(name::SEALED_TOKENS, &sealed).await?;
        }
        Ok(())
    }

    /// The end of a sync-like run: when it synced, why it didn't.
    async fn ended(&mut self, step: &Step) -> Result<()> {
        match step {
            Step::Wait { delay_ms } => {
                self.sync_error = Some("offline".to_owned());
                self.retry_sync_at = Some(self.clock.mono_ms().saturating_add(*delay_ms));
            }
            Step::Done(Outcome::Synced(_) | Outcome::VaultCreated { .. }) => {
                self.synced_at = Some(self.clock.now_ms());
                self.sync_error = None;
            }
            Step::Done(Outcome::SignedOut) => self.sync_error = Some("signed_out".to_owned()),
            Step::Done(Outcome::DeviceRevoked) => {
                // The core cleared the cache; nothing of the account stays here.
                self.forget_account().await?;
                self.sync_error = Some("device_revoked".to_owned());
            }
            _ => self.sync_error = Some("internal".to_owned()),
        }
        Ok(())
    }

    /// One sync run, when unlocked.
    pub async fn sync(&mut self) -> Result<Status> {
        if self.client.is_unlocked() {
            let step = self.client.start_sync();
            match self.run(step).await {
                Ok(step) => self.ended(&step).await?,
                Err(Failure(code)) => self.sync_error = Some(code),
            }
        }
        self.status().await
    }

    // --- Account ---

    async fn device_id(&mut self) -> Result<String> {
        if let Some(id) = self.keys.get(name::DEVICE_ID).await? {
            return Ok(id.expose_secret().clone());
        }
        let id = pwm_client::new_id()?;
        self.keys.set(name::DEVICE_ID, &id).await?;
        Ok(id)
    }

    async fn sign_up(&mut self, email: &str, password: &Secret<String>) -> Result<Secret<String>> {
        let device_id = self.device_id().await?;
        let start = self.client.start_signup(
            email,
            password.expose_secret(),
            &device_id,
            self.device.platform,
            &self.device.name,
        );
        let Step::Done(Outcome::SignedUp { secret_key }) = self.run(start).await? else {
            return Err(Failure::code("internal"));
        };
        self.signed_in(email, &secret_key).await?;
        Ok(secret_key)
    }

    async fn sign_in(
        &mut self,
        email: &str,
        password: &Secret<String>,
        secret_key: &Secret<String>,
    ) -> Result<()> {
        let device_id = self.device_id().await?;
        let start = self.client.start_login(
            email,
            password.expose_secret(),
            secret_key.expose_secret(),
            &device_id,
            self.device.platform,
            &self.device.name,
        );
        let Step::Done(Outcome::LoggedIn) = self.run(start).await? else {
            return Err(Failure::code("internal"));
        };
        self.signed_in(email, secret_key).await
    }

    /// After sign-up or sign-in: remember the account and make sure it has a vault.
    async fn signed_in(&mut self, email: &str, secret_key: &Secret<String>) -> Result<()> {
        self.keys.set(name::EMAIL, email).await?;
        self.keys
            .set(name::SECRET_KEY, secret_key.expose_secret())
            .await?;
        self.sync_error = None;
        let start = self.client.start_ensure_vault();
        match self.run(start).await {
            Ok(step) => self.ended(&step).await?,
            // Signed in all the same; the next sync makes the vault.
            Err(Failure(code)) => self.sync_error = Some(code),
        }
        Ok(())
    }

    /// After the server refused the refresh token (D50 §5): the master
    /// password only, since the email and Secret Key are here. The cache
    /// and its unsent changes stay.
    async fn sign_in_again(&mut self, password: &Secret<String>) -> Result<()> {
        let email = self.keys.get(name::EMAIL).await?;
        let secret_key = self.keys.get(name::SECRET_KEY).await?;
        let (Some(email), Some(secret_key)) = (email, secret_key) else {
            return Err(Failure::code("signed_out"));
        };
        let device_id = self.device_id().await?;
        // An empty name sends none: the device keeps its name.
        let start = self.client.start_login(
            email.expose_secret(),
            password.expose_secret(),
            secret_key.expose_secret(),
            &device_id,
            self.device.platform,
            "",
        );
        let Step::Done(Outcome::LoggedIn) = self.run(start).await? else {
            return Err(Failure::code("internal"));
        };
        self.sync_error = None;
        self.sync().await?;
        Ok(())
    }

    async fn unlock(&mut self, password: &Secret<String>) -> Result<()> {
        let secret_key = self
            .keys
            .get(name::SECRET_KEY)
            .await?
            .ok_or_else(|| Failure::code("signed_out"))?;
        if self.keys.secure() {
            let tokens = match self.keys.get(name::TOKENS).await? {
                Some(pair) => {
                    let pair: Value = serde_json::from_str(pair.expose_secret())
                        .map_err(|_| Failure::code("storage"))?;
                    Tokens::new(text(&pair["access"]), text(&pair["refresh"]))
                }
                None => Tokens::new(String::new(), String::new()),
            };
            self.client
                .unlock(password.expose_secret(), secret_key.expose_secret(), tokens)?;
        } else {
            let sealed = match self.keys.get(name::SEALED_TOKENS).await? {
                Some(sealed) => Some(
                    base64::engine::general_purpose::STANDARD
                        .decode(sealed.expose_secret())
                        .map_err(|_| Failure::code("storage"))?,
                ),
                None => None,
            };
            self.client.unlock_with_sealed_tokens(
                password.expose_secret(),
                secret_key.expose_secret(),
                sealed.as_deref(),
            )?;
        }
        let _ = self.client.purge_trash(self.clock.now_ms());
        Ok(())
    }

    /// Ends the session on the server (at most `SIGN_OUT_WAIT`, any failure
    /// ignored, D62), then wipes the account from this computer. The device
    /// id stays.
    async fn sign_out(&mut self) -> Result<()> {
        if self.client.is_unlocked() {
            let start = self.client.start_sign_out(None);
            let _ = tokio::time::timeout(SIGN_OUT_WAIT, self.run(start)).await;
        }
        self.client.sign_out()?;
        self.forget_account().await?;
        self.lock();
        self.synced_at = None;
        self.sync_error = None;
        Ok(())
    }

    async fn forget_account(&mut self) -> Result<()> {
        for key in [
            name::EMAIL,
            name::SECRET_KEY,
            name::TOKENS,
            name::SEALED_TOKENS,
        ] {
            self.keys.remove(key).await?;
        }
        Ok(())
    }

    async fn emergency_kit(&mut self) -> Result<Secret<Vec<u8>>> {
        let email = self.keys.get(name::EMAIL).await?;
        let secret_key = self.keys.get(name::SECRET_KEY).await?;
        let account_id = self.client.account_id()?;
        let (Some(email), Some(secret_key), Some(account_id)) = (email, secret_key, account_id)
        else {
            return Err(Failure::code("signed_out"));
        };
        let server = (self.settings.server_url != HOSTED_SERVER)
            .then_some(self.settings.server_url.as_str());
        Ok(pwm_client::emergency_kit(
            email.expose_secret(),
            secret_key.expose_secret(),
            server,
            &account_id,
            &local_date(),
            paper(),
        )?)
    }

    fn set_settings(&mut self, next: &Settings) -> Result<Settings> {
        let next = settings::check(next).map_err(|r| Failure::code(r.code()))?;
        // The account lives on one server: changing it means signing out first.
        if next.server_url != self.settings.server_url && self.client.account_id()?.is_some() {
            return Err(Failure::code("signed_in"));
        }
        if let Some(file) = &self.settings_file {
            file.save(&next)?;
        }
        self.settings = next.clone();
        Ok(next)
    }

    // --- The vault ---

    /// Every item that isn't trashed or a folder, as D18 JSON.
    fn stored_items(&self) -> Result<Vec<(String, Value)>> {
        let mut out = Vec::new();
        for vault_id in self.client.vault_ids()? {
            for json in self.client.items(&vault_id)? {
                let item: Value = serde_json::from_str(json.expose_secret())
                    .map_err(|_| Failure::code("invalid_item"))?;
                out.push((vault_id.clone(), item));
            }
        }
        Ok(out)
    }

    fn items(&self, query: &str) -> Result<Vec<ItemSummary>> {
        let needle = query.trim().to_lowercase();
        let mut list: Vec<ItemSummary> = self
            .stored_items()?
            .into_iter()
            .filter(|(_, item)| item["trashed_at"].is_null() && item["type"] != "folder")
            .map(|(vault_id, item)| {
                let login = login_of(&item);
                ItemSummary {
                    vault_id,
                    item_id: text(&item["id"]),
                    title: text(&item["title"]),
                    username: login.username,
                    url: login.url,
                    has_password: !login.password.expose_secret().is_empty(),
                    has_code: login.totp,
                }
            })
            .filter(|s| {
                needle.is_empty()
                    || [&s.title, &s.username, &s.url]
                        .iter()
                        .any(|v| v.to_lowercase().contains(&needle))
            })
            .collect();
        list.sort_by_key(|s| s.title.to_lowercase());
        Ok(list)
    }

    fn copy(&mut self, vault_id: &str, item_id: &str, field: CopyField) -> Result<()> {
        let value = match field {
            CopyField::Code => self
                .client
                .current_code(vault_id, item_id, self.clock.now_ms())?
                .map(|(code, _)| code)
                .unwrap_or_else(|| Secret::new(String::new())),
            CopyField::Username | CopyField::Password => {
                let item = self
                    .client
                    .item(vault_id, item_id)?
                    .ok_or_else(|| Failure::code("unknown_item"))?;
                let item: Value = serde_json::from_str(item.expose_secret())
                    .map_err(|_| Failure::code("invalid_item"))?;
                let login = login_of(&item);
                if field == CopyField::Username {
                    Secret::new(login.username)
                } else {
                    login.password
                }
            }
        };
        self.write_clipboard(value.expose_secret())
    }

    /// Copies, and clears the clipboard 30 seconds later (D34).
    fn write_clipboard(&mut self, value: &str) -> Result<()> {
        if value.is_empty() {
            return Err(Failure::code("empty"));
        }
        self.clipboard.write(value)?;
        self.clear_clipboard_at = Some(self.clock.mono_ms().saturating_add(CLEAR_AFTER_MS));
        Ok(())
    }

    // --- Asking another device (D46) ---

    async fn ask(&mut self, email: &str) -> Result<AskView> {
        self.cancel_ask().await;
        let email = email.trim().to_owned();
        let (ask, first) = ApprovalAsk::start(&email, self.device.platform)?;
        self.asking = Some(Asking {
            ask,
            email,
            code: None,
            approved: None,
        });
        let step = self.exchange(first).await?;
        self.advance(step).await
    }

    /// Called by the screen every 2 seconds while it shows the request.
    async fn poll(&mut self) -> Result<AskView> {
        let Some(asking) = &mut self.asking else {
            return Ok(view(AskState::Ended, None));
        };
        if asking.approved.is_some() {
            return Ok(view(AskState::Approved, None));
        }
        let request = asking.ask.poll()?;
        let step = self.exchange(request).await?;
        self.advance(step).await
    }

    /// Gives up, on the server too.
    async fn cancel_ask(&mut self) {
        if let Some(asking) = self.asking.take()
            && let Some(request) = asking.ask.cancel()
        {
            let base = base_url(&self.settings.server_url);
            let _ = tokio::time::timeout(CANCEL_WAIT, self.transport.send(&base, request)).await;
        }
    }

    async fn exchange(&mut self, request: HttpRequest) -> Result<AskStep> {
        let base = base_url(&self.settings.server_url);
        let response = self
            .transport
            .send(&base, request)
            .await
            .map_err(|_| Failure::code("network"))?;
        let asking = self
            .asking
            .as_mut()
            .ok_or_else(|| Failure::code("approval_ended"))?;
        Ok(asking.ask.receive(&response)?)
    }

    async fn advance(&mut self, mut step: AskStep) -> Result<AskView> {
        loop {
            let asking = self
                .asking
                .as_mut()
                .ok_or_else(|| Failure::code("approval_ended"))?;
            return Ok(match step {
                AskStep::Send(request) => {
                    step = self.exchange(request).await?;
                    continue;
                }
                AskStep::ShowCode { code } => {
                    asking.code = Some(code.clone());
                    view(AskState::Code, Some(code))
                }
                AskStep::Approved { secret_key } => {
                    asking.approved = Some(secret_key);
                    asking.code = None;
                    view(AskState::Approved, None)
                }
                AskStep::Wait { .. } => match &asking.code {
                    Some(code) => view(AskState::Code, Some(code.clone())),
                    None => view(AskState::Waiting, None),
                },
                AskStep::Ended => {
                    self.asking = None;
                    view(AskState::Ended, None)
                }
            });
        }
    }
}

fn view(state: AskState, code: Option<String>) -> AskView {
    AskView { state, code }
}

/// The generator's two kinds, with the extension's settings.
fn generate(mode: GenerateMode) -> Result<Secret<String>> {
    let generated = match mode {
        GenerateMode::Password => pwm_client::generate_password(&PasswordOptions {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            avoid_lookalikes: true,
        }),
        GenerateMode::Passphrase => pwm_client::generate_passphrase(&PassphraseOptions {
            words: 5,
            separator: "-".to_owned(),
            capitalize: false,
            add_digit: true,
        }),
    }
    .map_err(|_| Failure::code("internal"))?;
    Ok(generated.value)
}
