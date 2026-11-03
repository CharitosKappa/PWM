//! The health report (D22), devices (D26) and export (D24) as the apps use
//! them (D43 §6–8): across every vault of the unlocked account.

use std::fmt;

use pwm_crypto::{Secret, SecretKey};
use pwm_export::{ExportItem, ExportVault};
use pwm_store::Store;
use pwm_vault::{Item, ItemId};

use crate::client::parse_id;
use crate::{Client, ClientError};

/// One of the account's devices, its name opened. Times are Unix ms.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: String,
    /// `ios`, `android`, `macos`, `windows`, `linux` or `extension`.
    pub platform: String,
    /// `None` without a name, or when it does not open on this account.
    pub name: Option<String>,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub revoked_at: Option<i64>,
    /// The device asking.
    pub current: bool,
}

impl fmt::Debug for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceInfo")
            .field("platform", &self.platform)
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

/// A login whose password is weak: zxcvbn score below 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeakItem {
    pub item_id: String,
    pub score: u8,
}

/// The checks that never leave the device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HealthReport {
    pub weak: Vec<WeakItem>,
    /// Groups of logins that share one password; ids sorted in each group.
    pub reused: Vec<Vec<String>>,
}

/// A login whose password is in known breaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreachedItem {
    pub item_id: String,
    pub count: u64,
}

/// A breach check run by the platform (D22 §4): it fetches each of
/// [`prefixes`](Self::prefixes) from `HIBP_RANGE_URL` with the padding
/// header, straight from the device, and hands back each body.
pub struct BreachCheck(pwm_health::BreachCheck);

impl BreachCheck {
    /// The 5-character hash prefixes to request, each once.
    pub fn prefixes(&self) -> Vec<String> {
        self.0.prefixes()
    }

    pub fn add_response(&mut self, prefix: &str, body: &str) -> Result<(), ClientError> {
        Ok(self.0.add_response(prefix, body)?)
    }

    /// Breached logins; prefixes never answered count as not breached.
    pub fn finish(self) -> Vec<BreachedItem> {
        self.0
            .finish()
            .into_iter()
            .map(|b| BreachedItem {
                item_id: b.item_id.to_string(),
                count: b.count,
            })
            .collect()
    }
}

impl fmt::Debug for BreachCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BreachCheck").field(&self.0).finish()
    }
}

/// What to export (D24). `Pwmx` takes the export password.
#[derive(Clone, Copy)]
pub enum ExportFormat<'a> {
    Pwmx(&'a str),
    Json,
    Csv,
}

impl fmt::Debug for ExportFormat<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pwmx(_) => "Pwmx",
            Self::Json => "Json",
            Self::Csv => "Csv",
        })
    }
}

type VaultItems = Vec<(pwm_crypto::Id, Vec<(ItemId, Item)>)>;

impl<S: Store> Client<S> {
    /// Every item of every vault with its id, trashed ones included.
    fn every_item(&self) -> Result<VaultItems, ClientError> {
        let engine = self.engine_ref()?;
        self.vault_ids()?
            .iter()
            .map(|vault| {
                let id = parse_id(vault)?;
                Ok((id, engine.items(&id)?))
            })
            .collect()
    }

    /// Weak and reused passwords across every vault (D22): local only.
    pub fn health_report(&self) -> Result<HealthReport, ClientError> {
        let vaults = self.every_item()?;
        let items: Vec<(ItemId, &Item)> = vaults
            .iter()
            .flat_map(|(_, items)| items.iter().map(|(id, item)| (*id, item)))
            .collect();
        let local = pwm_health::check_local(&items)?;
        Ok(HealthReport {
            weak: local
                .weak
                .into_iter()
                .map(|w| WeakItem {
                    item_id: w.item_id.to_string(),
                    score: w.score,
                })
                .collect(),
            reused: local
                .reused
                .into_iter()
                .map(|group| group.iter().map(ToString::to_string).collect())
                .collect(),
        })
    }

    /// A breach check over every vault's logins, as they are now.
    pub fn breach_check(&self) -> Result<BreachCheck, ClientError> {
        let vaults = self.every_item()?;
        let items: Vec<(ItemId, &Item)> = vaults
            .iter()
            .flat_map(|(_, items)| items.iter().map(|(id, item)| (*id, item)))
            .collect();
        Ok(BreachCheck(pwm_health::BreachCheck::new(&items)?))
    }

    /// Checks the master password again, as unlocking does, before an
    /// export (D24). The grant stays in the client: the next
    /// [`export`](Self::export) uses it up; locking drops it; it lasts 5
    /// minutes.
    pub fn authorize_export(
        &mut self,
        password: &str,
        secret_key: &str,
        now_ms: i64,
    ) -> Result<(), ClientError> {
        let record = self.account_record()?;
        let secret_key = SecretKey::parse(secret_key).map_err(|_| ClientError::InvalidSecretKey)?;
        let grant = pwm_export::authorize_export(
            password,
            &secret_key,
            &record.account_id,
            &record.salt,
            &record.kdf_params,
            &record.sealed_keyset,
            now_ms,
        )
        .map_err(|_| ClientError::WrongPassword)?;
        self.export_grant = Some(grant);
        Ok(())
    }

    /// Every vault as a file (D24), using up the grant. A weak `.pwmx`
    /// password is refused first and keeps the grant. Works on every plan.
    pub fn export(
        &mut self,
        format: ExportFormat<'_>,
        now_ms: i64,
    ) -> Result<Secret<Vec<u8>>, ClientError> {
        if let ExportFormat::Pwmx(password) = format {
            pwm_export::check_export_password(password)?;
        }
        let vaults: Vec<ExportVault> = self
            .every_item()?
            .into_iter()
            .map(|(id, items)| ExportVault {
                id,
                items: items
                    .into_iter()
                    .map(|(id, item)| ExportItem { id, item })
                    .collect(),
            })
            .collect();
        let grant = self
            .export_grant
            .take()
            .ok_or(ClientError::ExportNotAuthorized)?;
        Ok(match format {
            ExportFormat::Pwmx(password) => {
                Secret::new(pwm_export::export_pwmx(grant, now_ms, &vaults, password)?)
            }
            ExportFormat::Json => pwm_export::export_json(grant, now_ms, &vaults)?,
            ExportFormat::Csv => pwm_export::export_csv(grant, now_ms, &vaults)?,
        })
    }
}
