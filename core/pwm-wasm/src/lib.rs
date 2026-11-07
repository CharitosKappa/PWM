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
    Client, ClientError, MemoryStore, Outcome, PasskeyCreateCheck, Platform, SaveOffer, Step,
    SyncEvent, Tokens,
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
    devices: Option<String>,
    approvals: Option<String>,
    billing: Option<String>,
    url: Option<String>,
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

    /// `signed_up`, `logged_in`, `vault_created`, `synced`, `signed_out`,
    /// `device_revoked`, `devices`, `device_updated`, `approvals`,
    /// `approval_updated`, `push_token_set`, `logged_out`, `billing` or
    /// `checkout_url`.
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

    /// After `devices`: a JSON array of `{id, platform, name, created_at,
    /// last_seen_at, revoked_at, current}`; `name` is null when it has none
    /// or it does not open.
    #[wasm_bindgen(getter)]
    pub fn devices(&self) -> Option<String> {
        self.devices.clone()
    }

    /// After `approvals`: a JSON array of `{id, platform, created_at, state,
    /// code}`; `code` is null until this device accepted and the new device
    /// revealed its key (D46).
    #[wasm_bindgen(getter)]
    pub fn approvals(&self) -> Option<String> {
        self.approvals.clone()
    }

    /// After `billing`: JSON `{plan, subscription, sells, token}`, where
    /// `subscription` is null or `{provider, plan, status, until_ms,
    /// canceling, price_locked_until_ms, also}` (D6 §7).
    #[wasm_bindgen(getter)]
    pub fn billing(&self) -> Option<String> {
        self.billing.clone()
    }

    /// After `checkout_url`: the Stripe Checkout page to open (D6).
    #[wasm_bindgen(getter)]
    pub fn url(&self) -> Option<String> {
        self.url.clone()
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
        devices: None,
        approvals: None,
        billing: None,
        url: None,
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
                Outcome::Devices(devices) => {
                    let list: Vec<serde_json::Value> = devices
                        .into_iter()
                        .map(|d| {
                            serde_json::json!({
                                "id": d.id, "platform": d.platform, "name": d.name,
                                "created_at": d.created_at, "last_seen_at": d.last_seen_at,
                                "revoked_at": d.revoked_at, "current": d.current,
                            })
                        })
                        .collect();
                    out.devices = Some(serde_json::Value::Array(list).to_string());
                    "devices"
                }
                Outcome::DeviceUpdated => "device_updated",
                Outcome::Approvals(approvals) => {
                    let list: Vec<serde_json::Value> = approvals
                        .into_iter()
                        .map(|a| {
                            serde_json::json!({
                                "id": a.id, "platform": a.platform, "created_at": a.created_at,
                                "state": a.state, "code": a.code,
                            })
                        })
                        .collect();
                    out.approvals = Some(serde_json::Value::Array(list).to_string());
                    "approvals"
                }
                Outcome::ApprovalUpdated => "approval_updated",
                Outcome::PushTokenSet => "push_token_set",
                Outcome::LoggedOut => "logged_out",
                Outcome::Billing(b) => {
                    let subscription = b.subscription.map(|s| {
                        serde_json::json!({
                            "provider": s.provider, "plan": s.plan, "status": s.status,
                            "until_ms": s.until_ms, "canceling": s.canceling,
                            "price_locked_until_ms": s.price_locked_until_ms, "also": s.also,
                        })
                    });
                    out.billing = Some(
                        serde_json::json!({
                            "plan": b.plan, "subscription": subscription,
                            "sells": b.sells, "token": b.token,
                        })
                        .to_string(),
                    );
                    "billing"
                }
                Outcome::CheckoutUrl(url) => {
                    out.url = Some(url);
                    "checkout_url"
                }
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
        device_name: &str,
    ) -> Result<WasmStep, JsValue> {
        let platform = platform(platform_name)?;
        self.inner
            .start_login(
                email,
                password,
                secret_key,
                device_id,
                platform,
                device_name,
            )
            .map(step)
            .map_err(js_error)
    }

    /// The plan, its subscription and what this server sells (D6); ends
    /// with `billing`.
    #[wasm_bindgen(js_name = startBillingStatus)]
    pub fn start_billing_status(&mut self) -> Result<WasmStep, JsValue> {
        self.inner
            .start_billing_status()
            .map(step)
            .map_err(js_error)
    }

    /// A Stripe Checkout page for `plan`; ends with `checkout_url`.
    #[wasm_bindgen(js_name = startCheckout)]
    pub fn start_checkout(&mut self, plan: &str) -> Result<WasmStep, JsValue> {
        self.inner.start_checkout(plan).map(step).map_err(js_error)
    }

    /// Cancels the Stripe subscription at the end of its period; ends with
    /// `billing`.
    #[wasm_bindgen(js_name = startCancelSubscription)]
    pub fn start_cancel_subscription(&mut self) -> Result<WasmStep, JsValue> {
        self.inner
            .start_cancel_subscription()
            .map(step)
            .map_err(js_error)
    }

    /// Lets a canceling Stripe subscription renew again; ends with `billing`.
    #[wasm_bindgen(js_name = startResumeSubscription)]
    pub fn start_resume_subscription(&mut self) -> Result<WasmStep, JsValue> {
        self.inner
            .start_resume_subscription()
            .map(step)
            .map_err(js_error)
    }

    /// The account's open device approval requests (D46); ends with `approvals`.
    #[wasm_bindgen(js_name = startListApprovals)]
    pub fn start_list_approvals(&mut self) -> Result<WasmStep, JsValue> {
        self.inner
            .start_list_approvals()
            .map(step)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = startAcceptApproval)]
    pub fn start_accept_approval(&mut self, approval_id: &str) -> Result<WasmStep, JsValue> {
        self.inner
            .start_accept_approval(approval_id)
            .map(step)
            .map_err(js_error)
    }

    /// After the user saw the same code on both devices.
    #[wasm_bindgen(js_name = startCompleteApproval)]
    pub fn start_complete_approval(
        &mut self,
        approval_id: &str,
        secret_key: &str,
    ) -> Result<WasmStep, JsValue> {
        self.inner
            .start_complete_approval(approval_id, secret_key)
            .map(step)
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = startDenyApproval)]
    pub fn start_deny_approval(&mut self, approval_id: &str) -> Result<WasmStep, JsValue> {
        self.inner
            .start_deny_approval(approval_id)
            .map(step)
            .map_err(js_error)
    }

    /// The account's devices; ends with `devices`.
    #[wasm_bindgen(js_name = startListDevices)]
    pub fn start_list_devices(&mut self) -> Result<WasmStep, JsValue> {
        self.inner.start_list_devices().map(step).map_err(js_error)
    }

    /// Renames one of the account's devices; ends with `device_updated`.
    #[wasm_bindgen(js_name = startRenameDevice)]
    pub fn start_rename_device(
        &mut self,
        device_id: &str,
        name: &str,
    ) -> Result<WasmStep, JsValue> {
        self.inner
            .start_rename_device(device_id, name)
            .map(step)
            .map_err(js_error)
    }

    /// Revokes another device; ends with `device_updated`.
    #[wasm_bindgen(js_name = startRevokeDevice)]
    pub fn start_revoke_device(&mut self, device_id: &str) -> Result<WasmStep, JsValue> {
        self.inner
            .start_revoke_device(device_id)
            .map(step)
            .map_err(js_error)
    }

    /// Weak and reused passwords across every vault (D22), as JSON:
    /// `{weak: [{item_id, score}], reused: [[item_id]]}`.
    #[wasm_bindgen(js_name = healthReport)]
    pub fn health_report(&self) -> Result<String, JsValue> {
        let report = self.inner.health_report().map_err(js_error)?;
        let weak: Vec<serde_json::Value> = report
            .weak
            .iter()
            .map(|w| serde_json::json!({ "item_id": w.item_id, "score": w.score }))
            .collect();
        Ok(serde_json::json!({ "weak": weak, "reused": report.reused }).to_string())
    }

    /// A breach check over the logins as they are now (D22 §4).
    #[wasm_bindgen(js_name = breachCheck)]
    pub fn breach_check(&self) -> Result<WasmBreachCheck, JsValue> {
        Ok(WasmBreachCheck {
            inner: Some(self.inner.breach_check().map_err(js_error)?),
        })
    }

    /// The master password again, before an export (D24); the grant stays
    /// in the core for the next `exportFile`, for 5 minutes.
    #[wasm_bindgen(js_name = authorizeExport)]
    pub fn authorize_export(
        &mut self,
        password: &str,
        secret_key: &str,
        now: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .authorize_export(password, secret_key, ms(now))
            .map_err(js_error)
    }

    /// Reads a picked file to import into `vault_id` (empty: the first
    /// vault) and returns the preview as JSON (D51), with the export's
    /// `password` and a KeePass `key_file` (empty: none): `{logins, notes, cards,
    /// identities, folders, trashed, left_out: [[reason, count]]}`. `mapping`
    /// is the CSV column mapping as JSON, or empty for the suggestion.
    #[wasm_bindgen(js_name = prepareImport)]
    // Each input is one thing the import screen collects.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_import(
        &mut self,
        vault_id: &str,
        format: &str,
        bytes: &[u8],
        password: &str,
        key_file: &[u8],
        mapping: &str,
        now: f64,
    ) -> Result<String, JsValue> {
        let mapping = if mapping.is_empty() {
            None
        } else {
            Some(mapping_from_json(mapping).map_err(js_error)?)
        };
        let preview = self
            .inner
            .prepare_import(
                (!vault_id.is_empty()).then_some(vault_id),
                format,
                bytes,
                (!password.is_empty()).then_some(password),
                (!key_file.is_empty()).then_some(key_file),
                mapping,
                ms(now),
            )
            .map_err(js_error)?;
        Ok(preview_json(&preview))
    }

    /// Saves the prepared import; sync afterwards. The preview as JSON.
    #[wasm_bindgen(js_name = finishImport)]
    pub fn finish_import(&mut self, now: f64) -> Result<String, JsValue> {
        let preview = self.inner.finish_import(ms(now)).map_err(js_error)?;
        Ok(preview_json(&preview))
    }

    #[wasm_bindgen(js_name = cancelImport)]
    pub fn cancel_import(&mut self) {
        self.inner.cancel_import();
    }

    /// Every vault as a file, using up the grant: `format` is `pwmx` (with
    /// `export_password`), `json` or `csv`.
    #[wasm_bindgen(js_name = exportFile)]
    pub fn export_file(
        &mut self,
        format: &str,
        export_password: &str,
        now: f64,
    ) -> Result<Vec<u8>, JsValue> {
        let format = match format {
            "pwmx" => pwm_client::ExportFormat::Pwmx(export_password),
            "json" => pwm_client::ExportFormat::Json,
            "csv" => pwm_client::ExportFormat::Csv,
            _ => return Err(js_error(ClientError::InvalidItem)),
        };
        let file = self.inner.export(format, ms(now)).map_err(js_error)?;
        Ok(file.expose_secret().clone())
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

    /// Ends this device's session on the server before signing out (D62),
    /// with the open session's tokens: the extension has none while locked,
    /// so locked it answers `locked`. Ends with `logged_out`.
    #[wasm_bindgen(js_name = startSignOut)]
    pub fn start_sign_out(&mut self) -> Result<WasmStep, JsValue> {
        self.inner.start_sign_out(None).map(step).map_err(js_error)
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

    /// One item as JSON with its id, or undefined (the manage page, D67).
    pub fn item(&self, vault_id: &str, item_id: &str) -> Result<Option<String>, JsValue> {
        Ok(self
            .inner
            .item(vault_id, item_id)
            .map_err(js_error)?
            .map(|json| json.expose_secret().clone()))
    }

    /// Moves an item to the trash (`trashed` true) or back out (D19).
    #[wasm_bindgen(js_name = setTrashed)]
    pub fn set_trashed(
        &mut self,
        vault_id: &str,
        item_id: &str,
        trashed: bool,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .set_trashed(vault_id, item_id, trashed, ms(now_ms))
            .map_err(js_error)
    }

    /// Deletes items in the trash for 30 days; called at unlock (D19).
    #[wasm_bindgen(js_name = purgeTrash)]
    pub fn purge_trash(&mut self, now_ms: f64) -> Result<u32, JsValue> {
        let purged = self.inner.purge_trash(ms(now_ms)).map_err(js_error)?;
        Ok(u32::try_from(purged).unwrap_or(u32::MAX))
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

    /// Touch ID unlock through the Mac app (D48 §5): a 32-byte key followed
    /// by the blob. The extension keeps the blob on disk and hands the key
    /// once to the helper, which keeps it behind Touch ID.
    #[wasm_bindgen(js_name = exportBrowserUnlock)]
    pub fn export_browser_unlock(&self, device_id: &str) -> Result<Vec<u8>, JsValue> {
        let unlock = self
            .inner
            .export_browser_unlock(device_id)
            .map_err(js_error)?;
        let mut out = unlock.key.expose_secret().to_vec();
        out.extend_from_slice(&unlock.blob);
        Ok(out)
    }

    /// Unlocks with the blob and the key the helper returned after Touch ID.
    #[wasm_bindgen(js_name = unlockBrowser)]
    pub fn unlock_browser(
        &mut self,
        blob: &[u8],
        key: &[u8],
        device_id: &str,
        sealed_tokens: Option<Vec<u8>>,
    ) -> Result<(), JsValue> {
        self.inner
            .unlock_browser(blob, key, device_id, sealed_tokens.as_deref())
            .map_err(js_error)
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

    /// Checks a passkey `create` (D37) before asking the user: JSON
    /// `{"kind": "ready" | "excluded" | "unsupported", "rp_id", "user_name"}`.
    /// `origin` is the browser-given one, never the page's.
    #[wasm_bindgen(js_name = passkeyCreateCheck)]
    pub fn passkey_create_check(
        &self,
        origin: &str,
        top_origin: Option<String>,
        request_json: &str,
    ) -> Result<String, JsValue> {
        let check = self
            .inner
            .passkey_create_check(origin, top_origin.as_deref(), request_json)
            .map_err(js_error)?;
        Ok(match check {
            PasskeyCreateCheck::Ready { rp_id, user_name } => serde_json::json!({
                "kind": "ready", "rp_id": rp_id, "user_name": user_name,
            }),
            PasskeyCreateCheck::Excluded => serde_json::json!({ "kind": "excluded" }),
            PasskeyCreateCheck::Unsupported => serde_json::json!({ "kind": "unsupported" }),
        }
        .to_string())
    }

    /// Creates and stores a passkey; returns `RegistrationResponseJSON`.
    #[wasm_bindgen(js_name = passkeyCreate)]
    pub fn passkey_create(
        &mut self,
        vault_id: &str,
        origin: &str,
        top_origin: Option<String>,
        request_json: &str,
        now_ms: f64,
    ) -> Result<String, JsValue> {
        self.inner
            .passkey_create(
                vault_id,
                origin,
                top_origin.as_deref(),
                request_json,
                ms(now_ms),
            )
            .map_err(js_error)
    }

    /// The passkeys a `get` may use, as a JSON array of `{vault_id,
    /// item_id, credential_id, user_name, title}` (never keys).
    #[wasm_bindgen(js_name = passkeyCandidates)]
    pub fn passkey_candidates(
        &self,
        origin: &str,
        top_origin: Option<String>,
        request_json: &str,
    ) -> Result<String, JsValue> {
        let list: Vec<serde_json::Value> = self
            .inner
            .passkey_candidates(origin, top_origin.as_deref(), request_json)
            .map_err(js_error)?
            .into_iter()
            .map(|c| {
                serde_json::json!({
                    "vault_id": c.vault_id, "item_id": c.item_id,
                    "credential_id": c.credential_id, "user_name": c.user_name,
                    "title": c.title,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(list).to_string())
    }

    /// Signs a `get` with the chosen passkey; `AuthenticationResponseJSON`.
    #[wasm_bindgen(js_name = passkeyGet)]
    pub fn passkey_get(
        &self,
        origin: &str,
        top_origin: Option<String>,
        request_json: &str,
        vault_id: &str,
        item_id: &str,
        credential_id: &str,
    ) -> Result<String, JsValue> {
        self.inner
            .passkey_get(
                origin,
                top_origin.as_deref(),
                request_json,
                vault_id,
                item_id,
                credential_id,
            )
            .map_err(js_error)
    }

    /// An "Autofill didn't work here" report (D38), as JSON, for the user
    /// to look at before sending. Never holds values.
    #[wasm_bindgen(js_name = autofillReport)]
    pub fn autofill_report(
        &self,
        page_url: &str,
        problem: &str,
        frames_json: &str,
        extension_version: &str,
        browser: &str,
    ) -> Result<String, JsValue> {
        self.inner
            .autofill_report(page_url, problem, frames_json, extension_version, browser)
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

/// The value to store in a login's `totp` for a pasted URI or bare secret
/// (D23 §3), or `invalid_totp`.
#[wasm_bindgen(js_name = normalizeTotp)]
pub fn normalize_totp(input: &str, title: &str) -> Result<String, JsValue> {
    pwm_client::normalize_totp(input, title)
        .map(|uri| uri.expose_secret().clone())
        .map_err(js_error)
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

/// The Emergency Kit as PDF bytes (D42). `paper` is `a4` or `letter`;
/// `server` is a self-hosted address, or undefined for the hosted service.
#[wasm_bindgen(js_name = emergencyKit)]
pub fn emergency_kit(
    email: &str,
    secret_key: &str,
    server: Option<String>,
    account_id: &str,
    created: &str,
    paper: &str,
) -> Result<Vec<u8>, JsValue> {
    let paper = match paper {
        "a4" => pwm_client::Paper::A4,
        "letter" => pwm_client::Paper::Letter,
        _ => return Err(js_error(ClientError::Internal)),
    };
    let pdf = pwm_client::emergency_kit(
        email,
        secret_key,
        server.as_deref(),
        account_id,
        created,
        paper,
    )
    .map_err(js_error)?;
    Ok(pdf.expose_secret().clone())
}

/// One breach check (D22 §4): fetch each prefix from `breachRangeUrl` with
/// the padding header, add each body, then finish.
#[wasm_bindgen]
pub struct WasmBreachCheck {
    inner: Option<pwm_client::BreachCheck>,
}

#[wasm_bindgen]
impl WasmBreachCheck {
    pub fn prefixes(&self) -> Vec<String> {
        self.inner
            .as_ref()
            .map(pwm_client::BreachCheck::prefixes)
            .unwrap_or_default()
    }

    #[wasm_bindgen(js_name = addResponse)]
    pub fn add_response(&mut self, prefix: &str, body: &str) -> Result<(), JsValue> {
        let check = self
            .inner
            .as_mut()
            .ok_or_else(|| js_error(ClientError::NotRunning))?;
        check.add_response(prefix, body).map_err(js_error)
    }

    /// JSON: `[{item_id, count}]`. The check is used up.
    pub fn finish(&mut self) -> String {
        let breached: Vec<serde_json::Value> = self
            .inner
            .take()
            .map(pwm_client::BreachCheck::finish)
            .unwrap_or_default()
            .iter()
            .map(|b| serde_json::json!({ "item_id": b.item_id, "count": b.count }))
            .collect();
        serde_json::Value::Array(breached).to_string()
    }
}

/// The HIBP range URL for a prefix (D22 §4).
fn preview_json(p: &pwm_client::ImportPreview) -> String {
    serde_json::json!({
        "logins": p.logins, "notes": p.notes, "cards": p.cards,
        "identities": p.identities, "folders": p.folders, "trashed": p.trashed,
        "left_out": p.left_out,
    })
    .to_string()
}

const MAPPING_FIELDS: [&str; 9] = [
    "title", "url", "username", "password", "notes", "totp", "folder", "favorite", "tags",
];

fn mapping_from_json(text: &str) -> Result<pwm_client::ColumnMapping, ClientError> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|_| ClientError::Import("import_unreadable"))?;
    let i = |k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as usize)
    };
    Ok(pwm_client::ColumnMapping {
        title: i("title"),
        url: i("url"),
        username: i("username"),
        password: i("password"),
        notes: i("notes"),
        totp: i("totp"),
        folder: i("folder"),
        favorite: i("favorite"),
        tags: i("tags"),
    })
}

fn mapping_json(m: &pwm_client::ColumnMapping) -> serde_json::Value {
    let values = [
        m.title, m.url, m.username, m.password, m.notes, m.totp, m.folder, m.favorite, m.tags,
    ];
    let mut map = serde_json::Map::new();
    for (key, value) in MAPPING_FIELDS.iter().zip(values) {
        map.insert(
            (*key).to_owned(),
            value.map_or(serde_json::Value::Null, Into::into),
        );
    }
    serde_json::Value::Object(map)
}

/// The format a picked file looks like, as JSON `{format, needs_password}`,
/// or nothing.
#[wasm_bindgen(js_name = detectImport)]
pub fn detect_import(file_name: &str, bytes: &[u8]) -> Option<String> {
    pwm_client::detect_import(file_name, bytes).map(|(format, needs_password)| {
        serde_json::json!({ "format": format, "needs_password": needs_password }).to_string()
    })
}

/// A CSV's header row and suggested columns as JSON `{headers, suggested,
/// rows}`.
#[wasm_bindgen(js_name = importColumns)]
pub fn import_columns(bytes: &[u8]) -> Result<String, JsValue> {
    let columns = pwm_client::import_columns(bytes).map_err(|e| js_error(ClientError::from(e)))?;
    Ok(serde_json::json!({
        "headers": columns.headers,
        "suggested": mapping_json(&columns.suggested),
        "rows": columns.rows,
    })
    .to_string())
}

#[wasm_bindgen(js_name = breachRangeUrl)]
pub fn breach_range_url(prefix: &str) -> String {
    format!("{}{prefix}", pwm_client::HIBP_RANGE_URL)
}

/// The new device's side of an approval (D46): `kind` is `send`, `wait`,
/// `code`, `approved` or `ended`.
#[wasm_bindgen]
pub struct WasmAskStep {
    kind: &'static str,
    request: Option<WasmStep>,
    delay_ms: f64,
    code: Option<String>,
    secret_key: Option<String>,
}

#[wasm_bindgen]
impl WasmAskStep {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.to_owned()
    }

    /// After `send`: the request, as a `send` step.
    pub fn request(&mut self) -> Option<WasmStep> {
        self.request.take()
    }

    #[wasm_bindgen(getter, js_name = delayMs)]
    pub fn delay_ms(&self) -> f64 {
        self.delay_ms
    }

    #[wasm_bindgen(getter)]
    pub fn code(&self) -> Option<String> {
        self.code.clone()
    }

    /// After `approved`: log in with it and the master password.
    #[wasm_bindgen(getter, js_name = secretKey)]
    pub fn secret_key(&self) -> Option<String> {
        self.secret_key.clone()
    }
}

fn send_step(request: pwm_client::HttpRequest) -> WasmStep {
    step(pwm_client::Step::Send(request))
}

#[wasm_bindgen]
pub struct WasmApprovalAsk {
    inner: pwm_client::ApprovalAsk,
    first: Option<pwm_client::HttpRequest>,
}

#[wasm_bindgen]
impl WasmApprovalAsk {
    #[wasm_bindgen(constructor)]
    pub fn new(email: &str, platform_name: &str) -> Result<WasmApprovalAsk, JsValue> {
        let (inner, first) =
            pwm_client::ApprovalAsk::start(email, platform(platform_name)?).map_err(js_error)?;
        Ok(Self {
            inner,
            first: Some(first),
        })
    }

    /// The request that makes it, as a `send` step; given once.
    #[wasm_bindgen(js_name = firstRequest)]
    pub fn first_request(&mut self) -> Option<WasmStep> {
        self.first.take().map(send_step)
    }

    pub fn poll(&mut self) -> Result<WasmStep, JsValue> {
        self.inner.poll().map(send_step).map_err(js_error)
    }

    pub fn cancel(&self) -> Option<WasmStep> {
        self.inner.cancel().map(send_step)
    }

    pub fn receive(
        &mut self,
        status: u16,
        headers: Vec<String>,
        body: Vec<u8>,
    ) -> Result<WasmAskStep, JsValue> {
        let headers = headers
            .chunks(2)
            .filter_map(|pair| match pair {
                [name, value] => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect();
        let response = pwm_client::HttpResponse::new(status, headers, body);
        let mut out = WasmAskStep {
            kind: "ended",
            request: None,
            delay_ms: 0.0,
            code: None,
            secret_key: None,
        };
        match self.inner.receive(&response).map_err(js_error)? {
            pwm_client::AskStep::Send(request) => {
                out.kind = "send";
                out.request = Some(send_step(request));
            }
            pwm_client::AskStep::Wait { delay_ms } => {
                out.kind = "wait";
                out.delay_ms = delay_ms as f64;
            }
            pwm_client::AskStep::ShowCode { code } => {
                out.kind = "code";
                out.code = Some(code);
            }
            pwm_client::AskStep::Approved { secret_key } => {
                out.kind = "approved";
                out.secret_key = Some(secret_key.expose_secret().clone());
            }
            pwm_client::AskStep::Ended => {}
        }
        Ok(out)
    }
}
