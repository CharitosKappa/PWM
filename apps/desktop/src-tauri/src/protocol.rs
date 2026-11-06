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
    Copy {
        vault_id: String,
        item_id: String,
        field: CopyField,
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
    /// Every other kind: the browser's requests, and the manage page's
    /// until M7.6.
    #[serde(other)]
    Unsupported,
}

/// The app's state (`Status` in `protocol.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Status {
    SignedOut,
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

/// A reply as the screens read it.
pub fn reply(result: Result<serde_json::Value, String>) -> serde_json::Value {
    match result {
        Ok(value) => serde_json::json!({ "ok": true, "value": value }),
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    }
}
