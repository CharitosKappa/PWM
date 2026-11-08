//! The screens' protocol (D66 §3): the same JSON as `ui/src/protocol.ts`.
//! A request is `{kind, ...fields}` in camelCase; a reply is
//! `{ok: true, value}` or `{ok: false, error}`. The browser's own requests
//! (filling, offers, passkeys, the report, Touch ID) are parsed as
//! `Unsupported` and answered `unsupported`.

use pwm_client::Secret;
use serde::{Deserialize, Deserializer, Serialize};

/// A request field that holds a secret: kept in a `Secret` from the start.
fn secret<'de, D: Deserializer<'de>>(d: D) -> Result<Secret<String>, D::Error> {
    String::deserialize(d).map(Secret::new)
}

/// What to copy from a login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CopyField {
    Username,
    Password,
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GenerateMode {
    Password,
    Passphrase,
}

/// Settings as the screens see them (`Settings` in `protocol.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub server_url: String,
    pub auto_lock_minutes: u32,
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Request {
    Status,
    SignUp {
        email: String,
        #[serde(deserialize_with = "secret")]
        password: Secret<String>,
    },
    SignIn {
        email: String,
        #[serde(deserialize_with = "secret")]
        password: Secret<String>,
        #[serde(deserialize_with = "secret")]
        secret_key: Secret<String>,
    },
    Unlock {
        #[serde(deserialize_with = "secret")]
        password: Secret<String>,
    },
    SignInAgain {
        #[serde(deserialize_with = "secret")]
        password: Secret<String>,
    },
    Lock,
    SignOut,
    Items {
        query: String,
    },
    /// A login's username, password or code (`field`), or any field by
    /// its JSON Pointer (`path`, D67 §1).
    Copy {
        vault_id: String,
        item_id: String,
        field: Option<CopyField>,
        path: Option<String>,
    },
    CopySecretKey,
    EmergencyKit,
    CopyText {
        #[serde(deserialize_with = "secret")]
        text: Secret<String>,
    },
    Generate {
        mode: GenerateMode,
    },
    Score {
        #[serde(deserialize_with = "secret")]
        password: Secret<String>,
        email: String,
    },
    Sync,
    Settings,
    SetSettings {
        settings: Settings,
    },
    Activity,
    AskApproval {
        email: String,
    },
    AskApprovalPoll,
    AskApprovalCancel,
    SignInApproved {
        #[serde(deserialize_with = "secret")]
        password: Secret<String>,
    },
    // The plan (D6; M9.1).
    Plan,
    Checkout {
        plan: String,
    },
    CancelPlan,
    ResumePlan,
    RefundPlan,
    /// Deleting the account (D74); the backend adds the Secret Key.
    DeleteAccount {
        #[serde(deserialize_with = "secret")]
        master_password: Secret<String>,
    },
    // The manage page (D44, D46, D51; M7.6).
    Devices,
    RenameDevice {
        device_id: String,
        name: String,
    },
    RevokeDevice {
        device_id: String,
    },
    Health,
    CheckLeaks,
    SetLeakCheck {
        on: bool,
    },
    Approvals,
    AcceptApproval {
        id: String,
    },
    CompleteApproval {
        id: String,
    },
    DenyApproval {
        id: String,
    },
    ExportVault {
        #[serde(deserialize_with = "secret")]
        master_password: Secret<String>,
        format: ExportFormat,
        /// For `pwmx` only.
        #[serde(deserialize_with = "secret")]
        export_password: Secret<String>,
    },
    /// The picked file as base64: an export, so a secret.
    ImportDetect {
        name: String,
        #[serde(deserialize_with = "secret")]
        file: Secret<String>,
    },
    ImportPrepare {
        #[serde(deserialize_with = "secret")]
        file: Secret<String>,
        format: String,
        /// The export's own password, or "".
        #[serde(deserialize_with = "secret")]
        password: Secret<String>,
        /// A KeePass key file as base64, or "".
        #[serde(deserialize_with = "secret")]
        key_file: Secret<String>,
        columns: Option<ImportMapping>,
    },
    ImportFinish,
    ImportCancel,
    // The item screens (D67; M7.7).
    Vault,
    Item {
        vault_id: String,
        item_id: String,
    },
    Reveal {
        vault_id: String,
        item_id: String,
        path: String,
    },
    ItemForEdit {
        vault_id: String,
        item_id: String,
    },
    /// The item is the editor's D18 JSON; it holds hidden values.
    SaveItem {
        vault_id: Option<String>,
        item: serde_json::Value,
    },
    TrashItem {
        vault_id: String,
        item_id: String,
    },
    RestoreItem {
        vault_id: String,
        item_id: String,
    },
    DeleteItem {
        vault_id: String,
        item_id: String,
    },
    NewFolder {
        title: String,
    },
    TotpSetup {
        #[serde(deserialize_with = "secret")]
        input: Secret<String>,
        title: String,
    },
    // The platform's biometric unlock (D68): Windows Hello on the desktop.
    TouchId,
    SetTouchId {
        on: bool,
    },
    UnlockTouchId,
    BiometricOffer,
    DismissBiometricOffer,
    // Browser extensions unlocking with Windows Hello (D69 §4).
    BrowserUnlock,
    SetBrowserUnlock {
        on: bool,
    },
    /// Every other kind: the browser's requests (filling, offers, passkeys,
    /// the report, Touch ID).
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Pwmx,
    Json,
    Csv,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Pwmx => "pwmx",
            Self::Json => "json",
            Self::Csv => "csv",
        }
    }
}

/// Which CSV column holds each field (`ImportMapping`); `None` leaves it out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportMapping {
    pub title: Option<usize>,
    pub url: Option<usize>,
    pub username: Option<usize>,
    pub password: Option<usize>,
    pub notes: Option<usize>,
    pub totp: Option<usize>,
    pub folder: Option<usize>,
    pub favorite: Option<usize>,
    pub tags: Option<usize>,
}

/// The account's plan (`PlanView`, D6 §7); times are Unix ms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub plan: String,
    pub subscription: Option<PlanSubscription>,
    pub sells: Vec<String>,
}

/// The subscription that gives the plan (`PlanView["subscription"]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSubscription {
    pub provider: String,
    pub plan: String,
    pub status: String,
    pub until_ms: i64,
    pub canceling: bool,
    pub price_locked_until_ms: Option<i64>,
    pub refundable_until_ms: Option<i64>,
    pub also: Vec<String>,
}

/// One of the account's devices (`DeviceView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub platform: String,
    pub name: Option<String>,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub revoked_at: Option<i64>,
    pub current: bool,
}

/// A login the health report names (`HealthItem`): never its password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthItem {
    pub vault_id: String,
    pub item_id: String,
    pub title: String,
    pub username: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeakedItem {
    #[serde(flatten)]
    pub item: HealthItem,
    pub count: u64,
}

/// The health report (`HealthView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthView {
    pub weak: Vec<HealthItem>,
    pub reused: Vec<Vec<HealthItem>>,
    /// `None` until the leak check has run in this unlocked session.
    pub leaked: Option<Vec<LeakedItem>>,
    pub leak_check: bool,
}

/// A new device asking to sign in (`ApprovalView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalView {
    pub id: String,
    pub platform: String,
    pub created_at: i64,
    pub state: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportColumnsView {
    pub headers: Vec<String>,
    pub suggested: ImportMapping,
    pub rows: usize,
}

/// What a picked file looks like (`ImportDetected`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDetected {
    pub format: Option<String>,
    pub needs_password: bool,
    pub columns: Option<ImportColumnsView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftOut {
    pub reason: String,
    pub count: usize,
}

/// What an import brings in (`ImportPreviewView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewView {
    pub logins: usize,
    pub notes: usize,
    pub cards: usize,
    pub identities: usize,
    pub folders: usize,
    pub trashed: usize,
    pub left_out: Vec<LeftOut>,
}

/// The app's state (`Status` in `protocol.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Status {
    SignedOut {
        /// Why the last account left this computer, when it wasn't signed
        /// out here: `account_deleted` (D74) or `device_revoked`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ended: Option<String>,
    },
    Locked {
        email: String,
        touch_id: bool,
    },
    Unlocked {
        email: String,
        synced_at: Option<i64>,
        sync_error: Option<String>,
        approvals: u32,
    },
}

/// One login in the list (`ItemSummary`): never a password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub vault_id: String,
    pub item_id: String,
    pub title: String,
    pub username: String,
    pub url: String,
    pub has_password: bool,
    pub has_code: bool,
}

/// Asking a signed-in device (`AskView`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskView {
    pub state: AskState,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AskState {
    Waiting,
    Code,
    Approved,
    Ended,
}

/// One row of the vault's list (`VaultRow`): never a hidden value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultRow {
    pub vault_id: String,
    pub item_id: String,
    pub r#type: String,
    pub title: String,
    pub subtitle: String,
    pub url: String,
    pub favorite: bool,
    pub folder_id: Option<String>,
    pub trashed: bool,
    pub has_password: bool,
    pub has_code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderView {
    pub vault_id: String,
    pub item_id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultView {
    pub items: Vec<VaultRow>,
    pub folders: Vec<FolderView>,
    /// The plan now includes one-time codes (D73 §1).
    pub codes_included: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotpNow {
    pub code: String,
    pub seconds_left: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyView {
    pub rp_id: String,
    pub user_name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PastPassword {
    pub saved_at: i64,
    pub path: String,
}

/// An item to show (`ItemView`): every hidden value left out (D67 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub vault_id: String,
    pub item_id: String,
    pub item: serde_json::Value,
    pub hidden: Vec<String>,
    pub read_only: bool,
    pub code: Option<TotpNow>,
    /// A secret, and a plan without codes (D73 §1).
    pub code_needs_plan: bool,
    pub passkeys: Vec<PasskeyView>,
    pub password_history: Vec<PastPassword>,
}

/// A reply as the screens read it.
pub fn reply(result: Result<serde_json::Value, String>) -> serde_json::Value {
    match result {
        Ok(value) => serde_json::json!({ "ok": true, "value": value }),
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    }
}
