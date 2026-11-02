//! WASM bindings of `pwm-client` for the browser extension (decision D31).
//! This crate only translates types and errors; all logic and its tests live
//! in `pwm-client`. The cache is a `MemoryStore`: the extension saves its
//! [`snapshot`](WasmClient::snapshot) to IndexedDB after each change and
//! loads it at startup.
//!
//! Errors are JS `Error`s with a `code` property, the stable code from
//! `ClientError::code` (for example `wrong_password`).
#![forbid(unsafe_code)]

use pwm_client::{
    Client, ClientError, MemoryStore, Outcome, Platform, SaveOffer, Step, SyncEvent, Tokens,
};
use wasm_bindgen::prelude::*;

fn js_error(err: ClientError) -> JsValue {
    let error = js_sys::Error::new(err.code());
    // Setting a property on a fresh object cannot fail.
    let _ = js_sys::Reflect::set(&error, &"code".into(), &err.code().into());
    error.into()
}

fn platform(name: &str) -> Result<Platform, JsValue> {
    Ok(match name {
        "ios" => Platform::Ios,
        "android" => Platform::Android,
        "macos" => Platform::Macos,
        "windows" => Platform::Windows,
        "linux" => Platform::Linux,
        "extension" => Platform::Extension,
        _ => return Err(js_error(ClientError::InvalidItem)),
    })
}

/// What the extension does next: `kind` is `send`, `wait` or `done`.
#[wasm_bindgen]
pub struct WasmStep {
    kind: &'static str,
    method: Option<String>,
    path: Option<String>,
    headers: Vec<String>,
    body: Option<Vec<u8>>,
    delay_ms: f64,
    outcome: Option<&'static str>,
    secret_key: Option<String>,
    vault_id: Option<String>,
    report: Vec<f64>,
}

#[wasm_bindgen]
impl WasmStep {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.to_owned()
    }

    #[wasm_bindgen(getter)]
    pub fn method(&self) -> Option<String> {
        self.method.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn path(&self) -> Option<String> {
        self.path.clone()
    }

    /// Header names and values, alternating: `[name, value, name, value]`.
    #[wasm_bindgen(getter)]
    pub fn headers(&self) -> Vec<String> {
        self.headers.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn body(&self) -> Option<Vec<u8>> {
        self.body.clone()
    }

    #[wasm_bindgen(getter, js_name = delayMs)]
    pub fn delay_ms(&self) -> f64 {
        self.delay_ms
    }

    /// `signed_up`, `logged_in`, `vault_created`, `synced`, `signed_out` or
    /// `device_revoked`.
    #[wasm_bindgen(getter)]
    pub fn outcome(&self) -> Option<String> {
        self.outcome.map(str::to_owned)
    }

    /// After sign-up only: show it on the Emergency Kit now.
    #[wasm_bindgen(getter, js_name = secretKey)]
    pub fn secret_key(&self) -> Option<String> {
        self.secret_key.clone()
    }

    #[wasm_bindgen(getter, js_name = vaultId)]
    pub fn vault_id(&self) -> Option<String> {
        self.vault_id.clone()
    }

    /// After a sync: `[pulled, pushed, conflicts, failed, resynced (0/1)]`.
    #[wasm_bindgen(getter)]
    pub fn report(&self) -> Vec<f64> {
        self.report.clone()
    }
}

fn step(step: Step) -> WasmStep {
    let mut out = WasmStep {
        kind: "done",
        method: None,
        path: None,
        headers: Vec::new(),
        body: None,
        delay_ms: 0.0,
        outcome: None,
        secret_key: None,
        vault_id: None,
        report: Vec::new(),
    };
    match step {
        Step::Send(request) => {
            out.kind = "send";
            out.method = Some(request.method.as_str().to_owned());
            out.path = Some(request.path.clone());
            out.headers = request
                .headers
                .iter()
                .flat_map(|(n, v)| [(*n).to_owned(), v.clone()])
                .collect();
            out.body = request.body.clone();
        }
        Step::Wait { delay_ms } => {
            out.kind = "wait";
            out.delay_ms = delay_ms as f64;
        }
        Step::Done(outcome) => {
            out.outcome = Some(match outcome {
                Outcome::SignedUp { secret_key } => {
                    out.secret_key = Some(secret_key.expose_secret().clone());
                    "signed_up"
                }
                Outcome::LoggedIn => "logged_in",
                Outcome::VaultCreated { vault_id } => {
                    out.vault_id = Some(vault_id);
                    "vault_created"
                }
                Outcome::Synced(r) => {
                    out.report = vec![
                        r.pulled as f64,
                        r.pushed as f64,
                        r.conflicts as f64,
                        r.failed as f64,
                        f64::from(u8::from(r.resynced)),
                    ];
                    "synced"
                }
                Outcome::SignedOut => "signed_out",
                Outcome::DeviceRevoked => "device_revoked",
            });
        }
    }
    out
}

/// Milliseconds since the epoch, as `Date.now()` gives them.
fn ms(now: f64) -> i64 {
    now as i64
}

/// The extension's handle on the core.
#[wasm_bindgen]
pub struct WasmClient {
    inner: Client<MemoryStore>,
}

#[wasm_bindgen]
impl WasmClient {
    /// A client with an empty cache.
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmClient {
        Self {
            inner: Client::new(MemoryStore::new()),
        }
    }

    /// A client over a cache saved with [`snapshot`](Self::snapshot).
    pub fn load(snapshot: &[u8]) -> Result<WasmClient, JsValue> {
        let store = MemoryStore::load(snapshot).map_err(|_| js_error(ClientError::Storage))?;
        Ok(Self {
            inner: Client::new(store),
        })
    }

    /// The whole cache (ciphertext and ids only) for IndexedDB.
    pub fn snapshot(&self) -> Vec<u8> {
        self.inner.store().snapshot()
    }

    #[wasm_bindgen(js_name = startSignup)]
    pub fn start_signup(
        &mut self,
        email: &str,
        password: &str,
        device_id: &str,
        platform_name: &str,
        device_name: &str,
    ) -> Result<WasmStep, JsValue> {
        let platform = platform(platform_name)?;
        self.inner
            .start_signup(email, password, device_id, platform, device_name)
            .map(step)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = startLogin)]
    pub fn start_login(
        &mut self,
        email: &str,
        password: &str,
        secret_key: &str,
        device_id: &str,
        platform_name: &str,
    ) -> Result<WasmStep, JsValue> {
        let platform = platform(platform_name)?;
        self.inner
            .start_login(email, password, secret_key, device_id, platform)
            .map(step)
            .map_err(js_error)
    }

    pub fn unlock(
        &mut self,
        password: &str,
        secret_key: &str,
        access: String,
        refresh: String,
    ) -> Result<(), JsValue> {
        self.inner
            .unlock(password, secret_key, Tokens::new(access, refresh))
            .map_err(js_error)
    }

    pub fn lock(&mut self) {
        self.inner.lock();
    }

    #[wasm_bindgen(js_name = isUnlocked)]
    pub fn is_unlocked(&self) -> bool {
        self.inner.is_unlocked()
    }

    #[wasm_bindgen(js_name = startSync)]
    pub fn start_sync(&mut self) -> Result<WasmStep, JsValue> {
        self.inner.start_sync().map(step).map_err(js_error)
    }

    #[wasm_bindgen(js_name = startCreateVault)]
    pub fn start_create_vault(&mut self) -> Result<WasmStep, JsValue> {
        self.inner.start_create_vault().map(step).map_err(js_error)
    }

    /// The answer to the last `send` step. `headers` alternate names and
    /// values, as in [`WasmStep::headers`].
    pub fn receive(
        &mut self,
        status: u16,
        headers: Vec<String>,
        body: Vec<u8>,
        now_ms: f64,
    ) -> Result<WasmStep, JsValue> {
        let headers = headers
            .chunks(2)
            .filter_map(|pair| match pair {
                [name, value] => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect();
        let response = pwm_client::HttpResponse::new(status, headers, body);
        self.inner
            .receive(response, ms(now_ms))
            .map(step)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = networkFailed)]
    pub fn network_failed(&mut self) -> Result<WasmStep, JsValue> {
        self.inner.network_failed().map(step).map_err(js_error)
    }

    pub fn cancel(&mut self) {
        self.inner.cancel();
    }

    /// New tokens to store, as `[access, refresh]`, if any arrived.
    #[wasm_bindgen(js_name = takeTokens)]
    pub fn take_tokens(&mut self) -> Option<Vec<String>> {
        self.inner
            .take_events()
            .into_iter()
            .rev()
            .find_map(|e| match e {
                SyncEvent::TokensRefreshed(t) => {
                    Some(vec![t.access().to_owned(), t.refresh().to_owned()])
                }
                SyncEvent::ItemFailed { .. } => None,
            })
    }

    #[wasm_bindgen(js_name = vaultIds)]
    pub fn vault_ids(&self) -> Result<Vec<String>, JsValue> {
        self.inner.vault_ids().map_err(js_error)
    }

    /// Items as D18 JSON with their id.
    pub fn items(&self, vault_id: &str) -> Result<Vec<String>, JsValue> {
        Ok(self
            .inner
            .items(vault_id)
            .map_err(js_error)?
            .into_iter()
            .map(|json| json.expose_secret().clone())
            .collect())
    }

    #[wasm_bindgen(js_name = saveItem)]
    pub fn save_item(
        &mut self,
        vault_id: &str,
        json: &str,
        now_ms: f64,
    ) -> Result<String, JsValue> {
        self.inner
            .save_item(vault_id, json, ms(now_ms))
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = deleteItem)]
    pub fn delete_item(&mut self, vault_id: &str, item_id: &str) -> Result<(), JsValue> {
        self.inner.delete_item(vault_id, item_id).map_err(js_error)
    }

    /// The logins this page (or the frame holding the fields) may get, best
    /// first, as a JSON array of `{vault_id, item_id, title, username,
    /// matched, decision}` (D33).
    #[wasm_bindgen(js_name = autofillMatches)]
    pub fn autofill_matches(
        &self,
        page_url: &str,
        frame_url: Option<String>,
    ) -> Result<String, JsValue> {
        let matches = self
            .inner
            .autofill_matches(page_url, frame_url.as_deref())
            .map_err(js_error)?;
        let json: Vec<serde_json::Value> = matches
            .into_iter()
            .map(|m| {
                serde_json::json!({
                    "vault_id": m.vault_id, "item_id": m.item_id, "title": m.title,
                    "username": m.username, "matched": m.matched, "decision": m.decision,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(json).to_string())
    }

    /// Field descriptions (JSON array, never values) in, `[{class, confidence}]` out.
    #[wasm_bindgen(js_name = classifyFields)]
    pub fn classify_fields(&self, fields_json: &str, page_url: &str) -> Result<String, JsValue> {
        self.inner
            .classify_fields(fields_json, page_url)
            .map_err(js_error)
    }

    /// Accepts a signed rules file; returns its version to store.
    #[wasm_bindgen(js_name = setAutofillRules)]
    pub fn set_autofill_rules(
        &mut self,
        bytes: &[u8],
        now_ms: f64,
        min_version: f64,
    ) -> Result<f64, JsValue> {
        self.inner
            .set_autofill_rules(bytes, ms(now_ms), min_version as u64)
            .map(|v| v as f64)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = autofillRecordStep)]
    pub fn autofill_record_step(
        &mut self,
        tab: &str,
        page_url: &str,
        item_id: &str,
        step: &str,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .autofill_record_step(tab, page_url, item_id, step, ms(now_ms))
            .map_err(js_error)
    }

    /// `[item_id, step]` to offer next on this tab, if any.
    #[wasm_bindgen(js_name = autofillNextStep)]
    pub fn autofill_next_step(
        &mut self,
        tab: &str,
        page_url: &str,
        now_ms: f64,
    ) -> Option<Vec<String>> {
        self.inner
            .autofill_next_step(tab, page_url, ms(now_ms))
            .map(|(item, step)| vec![item, step.to_owned()])
    }

    /// The unlocked session for `chrome.storage.session` (memory only, D34):
    /// a 32-byte key followed by the sealed blob. Never write it to disk.
    #[wasm_bindgen(js_name = exportSession)]
    pub fn export_session(&self) -> Result<Vec<u8>, JsValue> {
        let session = self.inner.export_session().map_err(js_error)?;
        let mut out = session.key.expose_secret().to_vec();
        out.extend_from_slice(&session.blob);
        Ok(out)
    }

    /// Unlocks from `exportSession` output after the worker restarted.
    #[wasm_bindgen(js_name = restoreSession)]
    pub fn restore_session(
        &mut self,
        session: &[u8],
        sealed_tokens: Option<Vec<u8>>,
    ) -> Result<(), JsValue> {
        if session.len() < 32 {
            return Err(js_error(ClientError::WrongPassword));
        }
        let (key, blob) = session.split_at(32);
        self.inner
            .restore_session(blob, key, sealed_tokens.as_deref())
            .map_err(js_error)
    }

    /// Tokens sealed under the account keyset, for `chrome.storage.local`.
    #[wasm_bindgen(js_name = sealTokens)]
    pub fn seal_tokens(&self, access: &str, refresh: &str) -> Result<Vec<u8>, JsValue> {
        self.inner.seal_tokens(access, refresh).map_err(js_error)
    }

    #[wasm_bindgen(js_name = unlockWithSealedTokens)]
    pub fn unlock_with_sealed_tokens(
        &mut self,
        password: &str,
        secret_key: &str,
        sealed_tokens: Option<Vec<u8>>,
    ) -> Result<(), JsValue> {
        self.inner
            .unlock_with_sealed_tokens(password, secret_key, sealed_tokens.as_deref())
            .map_err(js_error)
    }

    /// `[code, secondsLeft]` for a login with a one-time code.
    #[wasm_bindgen(js_name = currentCode)]
    pub fn current_code(
        &self,
        vault_id: &str,
        item_id: &str,
        now_ms: f64,
    ) -> Result<Option<Vec<String>>, JsValue> {
        Ok(self
            .inner
            .current_code(vault_id, item_id, ms(now_ms))
            .map_err(js_error)?
            .map(|(code, left)| vec![code.expose_secret().clone(), left.to_string()]))
    }

    /// Sync, then create a vault only if the account has none.
    #[wasm_bindgen(js_name = startEnsureVault)]
    pub fn start_ensure_vault(&mut self) -> Result<WasmStep, JsValue> {
        self.inner.start_ensure_vault().map(step).map_err(js_error)
    }

    #[wasm_bindgen(js_name = accountId)]
    pub fn account_id(&self) -> Result<Option<String>, JsValue> {
        self.inner.account_id().map_err(js_error)
    }

    /// What to offer after a sign-in (D36), as JSON: `{"kind": "nothing" |
    /// "save" | "update", "candidates": [{vault_id, item_id, title,
    /// username}]}`. The password stays here; it is not in the answer.
    #[wasm_bindgen(js_name = saveOffer)]
    pub fn save_offer(
        &self,
        page_url: &str,
        frame_url: Option<String>,
        username: &str,
        password: &str,
        new_password: Option<String>,
    ) -> Result<String, JsValue> {
        let offer = self
            .inner
            .save_offer(
                page_url,
                frame_url.as_deref(),
                username,
                password,
                new_password.as_deref(),
            )
            .map_err(js_error)?;
        let (kind, candidates) = match offer {
            SaveOffer::Nothing => ("nothing", Vec::new()),
            SaveOffer::Save => ("save", Vec::new()),
            SaveOffer::Update(list) => ("update", list),
        };
        let candidates: Vec<serde_json::Value> = candidates
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "vault_id": c.vault_id, "item_id": c.item_id,
                    "title": c.title, "username": c.username,
                })
            })
            .collect();
        Ok(serde_json::json!({ "kind": kind, "candidates": candidates }).to_string())
    }

    /// Saves a new login for the site of `page_url`; returns its id.
    #[wasm_bindgen(js_name = saveLogin)]
    pub fn save_login(
        &mut self,
        vault_id: &str,
        page_url: &str,
        title: &str,
        username: &str,
        password: &str,
        now_ms: f64,
    ) -> Result<String, JsValue> {
        self.inner
            .save_login(vault_id, page_url, title, username, password, ms(now_ms))
            .map_err(js_error)
    }

    /// Replaces a login's password; the old one goes to history.
    #[wasm_bindgen(js_name = updatePassword)]
    pub fn update_password(
        &mut self,
        vault_id: &str,
        item_id: &str,
        password: &str,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .update_password(vault_id, item_id, password, ms(now_ms))
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = autofillForgetTab)]
    pub fn autofill_forget_tab(&mut self, tab: &str) {
        self.inner.autofill_forget_tab(tab);
    }
}

impl Default for WasmClient {
    fn default() -> Self {
        Self::new()
    }
}

/// A new random id for an item or a device.
#[wasm_bindgen(js_name = newId)]
pub fn new_id() -> Result<String, JsValue> {
    pwm_client::new_id().map_err(js_error)
}

#[wasm_bindgen(js_name = generatePassword)]
pub fn generate_password(
    length: u32,
    lowercase: bool,
    uppercase: bool,
    digits: bool,
    symbols: bool,
    avoid_lookalikes: bool,
) -> Result<String, JsValue> {
    let options = pwm_client::PasswordOptions {
        length: length as usize,
        lowercase,
        uppercase,
        digits,
        symbols,
        avoid_lookalikes,
    };
    pwm_client::generate_password(&options)
        .map(|g| g.value.expose_secret().clone())
        .map_err(|_| js_error(ClientError::InvalidItem))
}

#[wasm_bindgen(js_name = generatePassphrase)]
pub fn generate_passphrase(
    words: u32,
    separator: String,
    capitalize: bool,
    add_digit: bool,
) -> Result<String, JsValue> {
    let options = pwm_client::PassphraseOptions {
        words: words as usize,
        separator,
        capitalize,
        add_digit,
    };
    pwm_client::generate_passphrase(&options)
        .map(|g| g.value.expose_secret().clone())
        .map_err(|_| js_error(ClientError::InvalidItem))
}

/// The 0-4 score of `password` (D21).
#[wasm_bindgen(js_name = passwordScore)]
pub fn password_score(password: &str, context: Vec<String>) -> u8 {
    let context: Vec<&str> = context.iter().map(String::as_str).collect();
    pwm_client::strength(password, &context).score
}
