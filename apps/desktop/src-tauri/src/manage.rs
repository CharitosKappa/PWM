//! The manage page's requests on the desktop (M7.6, D66 §1), as the
//! extension answers them (D44, D46, D51): the account's devices, approving
//! a new one, the health report with its leak check, export and import.
//! Leak results stay in memory until the vault locks; an export is checked
//! and made in one request; an imported file is read in memory only.

use std::collections::HashMap;

use base64::Engine;
use pwm_client::{ClientError, ColumnMapping, ExportFormat as CoreFormat, Outcome, Secret, Step};
use serde_json::Value;

use crate::backend::{Backend, Clock, Failure, local_date};
use crate::clipboard::Clipboard;
use crate::http::Transport;
use crate::keys::{KeyStore, name};
use crate::protocol::{
    ApprovalView, DeviceView, ExportFormat, HealthItem, HealthView, ImportColumnsView,
    ImportDetected, ImportMapping, ImportPreviewView, LeakedItem, LeftOut, PlanSubscription,
    PlanView,
};

type Result<T> = std::result::Result<T, Failure>;

/// The largest file the page may send, as in the extension: the core's own
/// limit is 50 MB (D51 §7).
pub const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;
/// An export password must score at least this (D24, as the extension).
const MIN_EXPORT_SCORE: u8 = 3;

fn mapping(m: &ImportMapping) -> ColumnMapping {
    ColumnMapping {
        title: m.title,
        url: m.url,
        username: m.username,
        password: m.password,
        notes: m.notes,
        totp: m.totp,
        folder: m.folder,
        favorite: m.favorite,
        tags: m.tags,
    }
}

fn view(m: &ColumnMapping) -> ImportMapping {
    ImportMapping {
        title: m.title,
        url: m.url,
        username: m.username,
        password: m.password,
        notes: m.notes,
        totp: m.totp,
        folder: m.folder,
        favorite: m.favorite,
        tags: m.tags,
    }
}

fn preview(p: &pwm_client::ImportPreview) -> ImportPreviewView {
    ImportPreviewView {
        logins: p.logins,
        notes: p.notes,
        cards: p.cards,
        identities: p.identities,
        folders: p.folders,
        trashed: p.trashed,
        left_out: p
            .left_out
            .iter()
            .map(|(reason, count)| LeftOut {
                reason: reason.clone(),
                count: *count,
            })
            .collect(),
    }
}

/// A picked file's base64, checked against the size limit first.
fn file_bytes(file: &Secret<String>) -> Result<Secret<Vec<u8>>> {
    // Base64 is 4 characters for every 3 bytes.
    if file.expose_secret().len() > MAX_IMPORT_BYTES.div_ceil(3) * 4 {
        return Err(Failure::code("import_too_large"));
    }
    base64::engine::general_purpose::STANDARD
        .decode(file.expose_secret())
        .map(Secret::new)
        .map_err(|_| Failure::code("import_unreadable"))
}

fn by_title(a: &HealthItem, b: &HealthItem) -> std::cmp::Ordering {
    a.title
        .to_lowercase()
        .cmp(&b.title.to_lowercase())
        .then_with(|| a.item_id.cmp(&b.item_id))
}

impl<T: Transport, K: KeyStore, C: Clipboard, Q: Clock> Backend<T, K, C, Q> {
    /// A server conversation that must end with `wanted`; anything else
    /// (offline, revoked, signed out) is handled as a sync's end would be.
    async fn conversation(
        &mut self,
        start: std::result::Result<Step, ClientError>,
        wanted: fn(&Outcome) -> bool,
    ) -> Result<Outcome> {
        let step = self.run(start).await?;
        if let Step::Done(outcome) = &step
            && wanted(outcome)
        {
            let Step::Done(outcome) = step else {
                unreachable!("matched above");
            };
            return Ok(outcome);
        }
        self.ended(&step).await?;
        Err(Failure::code(match step {
            Step::Wait { .. } => "network",
            Step::Done(Outcome::SignedOut) => "signed_out",
            Step::Done(Outcome::DeviceRevoked) => "device_revoked",
            _ => "internal",
        }))
    }

    // --- The plan (D6) ---

    pub(crate) async fn plan(&mut self) -> Result<PlanView> {
        let start = self.client.start_billing_status();
        self.plan_conversation(start).await
    }

    /// Stripe's Checkout page; the screen opens it in the default browser.
    pub(crate) async fn checkout(&mut self, plan: &str) -> Result<String> {
        let start = self.client.start_checkout(plan);
        let Outcome::CheckoutUrl(url) = self
            .conversation(start, |o| matches!(o, Outcome::CheckoutUrl(_)))
            .await?
        else {
            unreachable!("checked by the conversation");
        };
        if !url.starts_with("https://") {
            return Err(Failure::code("internal"));
        }
        Ok(url)
    }

    /// One click either way (PRICING.md §4).
    pub(crate) async fn set_plan_canceling(&mut self, canceling: bool) -> Result<PlanView> {
        let start = if canceling {
            self.client.start_cancel_subscription()
        } else {
            self.client.start_resume_subscription()
        };
        self.plan_conversation(start).await
    }

    async fn plan_conversation(
        &mut self,
        start: std::result::Result<Step, ClientError>,
    ) -> Result<PlanView> {
        let Outcome::Billing(info) = self
            .conversation(start, |o| matches!(o, Outcome::Billing(_)))
            .await?
        else {
            unreachable!("checked by the conversation");
        };
        Ok(PlanView {
            plan: info.plan,
            subscription: info.subscription.map(|s| PlanSubscription {
                provider: s.provider,
                plan: s.plan,
                status: s.status,
                until_ms: s.until_ms,
                canceling: s.canceling,
                price_locked_until_ms: s.price_locked_until_ms,
                also: s.also,
            }),
            sells: info.sells,
        })
    }

    // --- Devices ---

    pub(crate) async fn devices(&mut self) -> Result<Vec<DeviceView>> {
        let start = self.client.start_list_devices();
        let Outcome::Devices(list) = self
            .conversation(start, |o| matches!(o, Outcome::Devices(_)))
            .await?
        else {
            unreachable!("checked by the conversation");
        };
        Ok(list
            .into_iter()
            .map(|d| DeviceView {
                id: d.id,
                platform: d.platform,
                name: d.name,
                created_at: d.created_at,
                last_seen_at: d.last_seen_at,
                revoked_at: d.revoked_at,
                current: d.current,
            })
            .collect())
    }

    pub(crate) async fn rename_device(&mut self, id: &str, name: &str) -> Result<Vec<DeviceView>> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Failure::code("invalid_device_name"));
        }
        let start = self.client.start_rename_device(id, name);
        self.conversation(start, |o| matches!(o, Outcome::DeviceUpdated))
            .await?;
        self.devices().await
    }

    /// Another device only: this one leaves with Sign out.
    pub(crate) async fn revoke_device(&mut self, id: &str) -> Result<Vec<DeviceView>> {
        if self
            .devices()
            .await?
            .iter()
            .any(|d| d.current && d.id == id)
        {
            return Err(Failure::code("this_device"));
        }
        let start = self.client.start_revoke_device(id);
        self.conversation(start, |o| matches!(o, Outcome::DeviceUpdated))
            .await?;
        self.devices().await
    }

    // --- Approving a new device (D46) ---

    pub(crate) async fn approvals(&mut self) -> Result<Vec<ApprovalView>> {
        let start = self.client.start_list_approvals();
        let Outcome::Approvals(list) = self
            .conversation(start, |o| matches!(o, Outcome::Approvals(_)))
            .await?
        else {
            unreachable!("checked by the conversation");
        };
        self.pending_approvals = u32::try_from(list.len()).unwrap_or(u32::MAX);
        Ok(list
            .into_iter()
            .map(|a| ApprovalView {
                id: a.id,
                platform: a.platform,
                created_at: a.created_at,
                state: a.state,
                code: a.code,
            })
            .collect())
    }

    pub(crate) async fn accept_approval(&mut self, id: &str) -> Result<Vec<ApprovalView>> {
        let start = self.client.start_accept_approval(id);
        self.conversation(start, |o| matches!(o, Outcome::ApprovalUpdated))
            .await?;
        self.approvals().await
    }

    /// After the user saw the same code on both devices: sends this
    /// device's Secret Key, sealed to the new one.
    pub(crate) async fn complete_approval(&mut self, id: &str) -> Result<Vec<ApprovalView>> {
        let secret_key = self
            .keys
            .get(name::SECRET_KEY)
            .await?
            .ok_or_else(|| Failure::code("signed_out"))?;
        let start = self
            .client
            .start_complete_approval(id, secret_key.expose_secret());
        self.conversation(start, |o| matches!(o, Outcome::ApprovalUpdated))
            .await?;
        self.approvals().await
    }

    pub(crate) async fn deny_approval(&mut self, id: &str) -> Result<Vec<ApprovalView>> {
        let start = self.client.start_deny_approval(id);
        self.conversation(start, |o| matches!(o, Outcome::ApprovalUpdated))
            .await?;
        self.approvals().await
    }

    /// After a sync: how many new devices are asking, for the vault's notice.
    pub(crate) async fn refresh_approvals(&mut self) {
        // Shown at the next sync if it fails now; the vault works without it.
        let _ = self.approvals().await;
    }

    // --- Health (D22, D44) ---

    /// Every login not in the trash, by id.
    fn logins_by_id(&self) -> Result<HashMap<String, HealthItem>> {
        let mut out = HashMap::new();
        for (vault_id, item) in self.stored_items()? {
            if item["type"] != "login" || !item["trashed_at"].is_null() {
                continue;
            }
            let Some(id) = item["id"].as_str() else {
                continue;
            };
            let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
            out.insert(
                id.to_owned(),
                HealthItem {
                    vault_id,
                    item_id: id.to_owned(),
                    title: text(&item["title"]),
                    username: text(&item["login"]["username"]),
                    url: text(&item["login"]["urls"][0]["url"]),
                },
            );
        }
        Ok(out)
    }

    pub(crate) fn health(&self) -> Result<HealthView> {
        let items = self.logins_by_id()?;
        let report = self.client.health_report()?;
        let named = |ids: &mut dyn Iterator<Item = &String>| -> Vec<HealthItem> {
            let mut list: Vec<HealthItem> = ids.filter_map(|id| items.get(id).cloned()).collect();
            list.sort_by(by_title);
            list
        };
        let leaked = self.leaked.as_ref().map(|leaked| {
            let mut list: Vec<LeakedItem> = leaked
                .iter()
                .filter_map(|(id, count)| {
                    items.get(id).map(|item| LeakedItem {
                        item: item.clone(),
                        count: *count,
                    })
                })
                .collect();
            list.sort_by(|a, b| by_title(&a.item, &b.item));
            list
        });
        Ok(HealthView {
            weak: named(&mut report.weak.iter().map(|w| &w.item_id)),
            reused: report
                .reused
                .iter()
                .map(|group| named(&mut group.iter()))
                .filter(|group| group.len() > 1)
                .collect(),
            leaked,
            leak_check: self.leak_check,
        })
    }

    pub(crate) fn set_leak_check(&mut self, on: bool) -> Result<bool> {
        if let Some(file) = &self.settings_file {
            file.save(&self.settings, on)?;
        }
        self.leak_check = on;
        if !on {
            self.leaked = None;
        }
        Ok(on)
    }

    /// The leak check (D22 §4): only 5-character hash prefixes leave, one
    /// request each, straight to Have I Been Pwned with padding and nothing
    /// that names the account.
    pub(crate) async fn check_leaks(&mut self) -> Result<HealthView> {
        if !self.leak_check {
            return Err(Failure::code("leak_check_off"));
        }
        let mut check = self.client.breach_check()?;
        let (header, value) = pwm_client::ADD_PADDING_HEADER;
        for prefix in check.prefixes() {
            let url = format!("{}{prefix}", pwm_client::HIBP_RANGE_URL);
            let (status, body) = self
                .transport
                .get(&url, &[(header, value)])
                .await
                .map_err(|_| Failure::code("network"))?;
            if status != 200 {
                return Err(Failure::code("breach_check"));
            }
            check.add_response(&prefix, &body)?;
        }
        self.leaked = Some(
            check
                .finish()
                .into_iter()
                .map(|b| (b.item_id, b.count))
                .collect(),
        );
        self.health()
    }

    // --- Export (D24, D44 §6) ---

    /// Checks the master password and makes the file in one go. A weak
    /// export password is refused first, so no grant is ever left unused.
    pub(crate) async fn export_vault(
        &mut self,
        master_password: &Secret<String>,
        format: ExportFormat,
        export_password: &Secret<String>,
    ) -> Result<(String, String)> {
        if format == ExportFormat::Pwmx
            && pwm_client::strength(export_password.expose_secret(), &[]).score < MIN_EXPORT_SCORE
        {
            return Err(Failure::code("weak_export_password"));
        }
        let secret_key = self
            .keys
            .get(name::SECRET_KEY)
            .await?
            .ok_or_else(|| Failure::code("signed_out"))?;
        let now = self.clock.now_ms();
        self.client.authorize_export(
            master_password.expose_secret(),
            secret_key.expose_secret(),
            now,
        )?;
        let core_format = match format {
            ExportFormat::Pwmx => CoreFormat::Pwmx(export_password.expose_secret()),
            ExportFormat::Json => CoreFormat::Json,
            ExportFormat::Csv => CoreFormat::Csv,
        };
        let file = self.client.export(core_format, now)?;
        Ok((
            base64::engine::general_purpose::STANDARD.encode(file.expose_secret()),
            format!("PWM export {}.{}", local_date(), format.extension()),
        ))
    }

    // --- Import (D51 §9) ---

    pub(crate) fn import_detect(
        &self,
        file_name: &str,
        file: &Secret<String>,
    ) -> Result<ImportDetected> {
        let bytes = file_bytes(file)?;
        let found = pwm_client::detect_import(file_name, bytes.expose_secret());
        let columns = match &found {
            Some((format, _)) if format == "csv" => {
                let columns = pwm_client::import_columns(bytes.expose_secret())
                    .map_err(|e| Failure::from(ClientError::from(e)))?;
                Some(ImportColumnsView {
                    headers: columns.headers,
                    suggested: view(&columns.suggested),
                    rows: columns.rows,
                })
            }
            _ => None,
        };
        Ok(ImportDetected {
            needs_password: found.as_ref().is_some_and(|(_, needs)| *needs),
            format: found.map(|(format, _)| format),
            columns,
        })
    }

    pub(crate) fn import_prepare(
        &mut self,
        file: &Secret<String>,
        format: &str,
        password: &Secret<String>,
        key_file: &Secret<String>,
        columns: Option<&ImportMapping>,
    ) -> Result<ImportPreviewView> {
        let bytes = file_bytes(file)?;
        let key = if key_file.expose_secret().is_empty() {
            None
        } else {
            Some(file_bytes(key_file)?)
        };
        let password = password.expose_secret();
        let found = self.client.prepare_import(
            None,
            format,
            bytes.expose_secret(),
            (!password.is_empty()).then_some(password.as_str()),
            key.as_ref().map(|k| k.expose_secret().as_slice()),
            columns.map(mapping),
            self.clock.now_ms(),
        )?;
        Ok(preview(&found))
    }

    /// Saves what was previewed; the app syncs next.
    pub(crate) fn import_finish(&mut self) -> Result<ImportPreviewView> {
        let done = self.client.finish_import(self.clock.now_ms())?;
        Ok(preview(&done))
    }

    pub(crate) fn import_cancel(&mut self) {
        self.client.cancel_import();
    }
}
