//! The one API the apps and the browser extension use (decision D31):
//! account (sign-up, login, unlock, lock), vaults and items, sync, autofill
//! (D33) and the generator. The UniFFI (`pwm-ffi`) and WASM (`pwm-wasm`) crates only
//! translate its types. Every server conversation is a series of steps the
//! platform sends with its own HTTP stack.
#![forbid(unsafe_code)]

mod approval;
mod auth;
mod autofill;
mod billing;
mod biometric;
mod client;
mod error;
mod import;
mod kit;
mod passkey;
mod save;
mod session;
mod tools;
mod totp;

pub use approval::{ApprovalAsk, ApprovalRequest, AskStep, POLL_EVERY_MS};
pub use auth::id_text;
pub use autofill::AutofillMatch;
pub use billing::{BillingInfo, SubscriptionInfo};
pub use biometric::{BiometricUnlock, BrowserUnlock, WindowsHelloUnlock, windows_hello_challenge};
pub use client::{Client, Outcome, PushToken, Step, new_id};
pub use error::ClientError;
pub use import::{ImportPreview, detect_import};
pub use kit::{ScannedKit, emergency_kit, scan_kit};
pub use passkey::{
    AndroidPasskeyCaller, CredentialIdentity, HashedAssertion, HashedPasskey, PasskeyCandidate,
    PasskeyCreateCheck, asset_links_includes, passkey_asset_links_url,
};
pub use pwm_autofill::{AppLinkError, app_link};
/// Which platform key answers the desktop's unlock challenge (D68, D71).
pub use pwm_crypto::Signer;
pub use pwm_crypto::{KdfParams, Secret};
pub use pwm_generator::{
    PassphraseOptions, PasswordOptions, Strength, generate_passphrase, generate_password, strength,
};
pub use pwm_health::{ADD_PADDING_HEADER, HIBP_RANGE_URL};
pub use pwm_import::{ColumnMapping, Columns as ImportColumns, csv_columns as import_columns};
pub use pwm_kit::Paper;
pub use pwm_store::{MemoryStore, SNAPSHOT_VERSION, Store};
pub use pwm_sync::{HttpRequest, HttpResponse, Method, SyncEvent, SyncReport, Tokens};
pub use save::{SaveCandidate, SaveOffer};
pub use session::SessionExport;
pub use tools::{BreachCheck, BreachedItem, DeviceInfo, ExportFormat, HealthReport, WeakItem};
pub use totp::normalize_totp;

/// The platforms a device can be (D26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Android,
    Macos,
    Windows,
    Linux,
    Extension,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Extension => "extension",
        }
    }
}

/// This device as the server knows it.
pub(crate) struct Device {
    pub id: pwm_crypto::Id,
    pub platform: Platform,
    pub name: String,
}
