//! PWM sync server (D25–D27, D29, D30): an axum API over Postgres. It stores
//! ciphertext, sizes, ids and times only, plus the account email.
#![forbid(unsafe_code)]

mod account;
mod api;
mod approvals;
mod auth;
mod billing;
mod config;
mod db;
mod deletion;
mod email;
mod error;
mod items;
mod limits;
mod logging;
mod maintenance;
mod push;
mod reports;
mod routes;
mod session;
mod vaults;

pub use api::AppState;
pub use billing::{
    AppleConfig, Billing, BillingConfig, Mailer, OutboxMessage, PLAY_SCOPE, PlayConfig,
    STRIPE_VERSION, StripeConfig, record_price_change,
};
pub use config::{Config, DEFAULT_LISTEN, generate_server_secrets};
pub use db::{MIGRATOR, connect, healthcheck, mark_restored, migrate};
pub use deletion::{DELETED_ACCOUNTS_KEPT_DAYS, DeletionLog, purge_deletions, redelete};
pub use email::normalize_email;
pub use error::ServerError;
pub use items::{MAX_ITEM_BLOB_BYTES, MAX_ITEMS_PER_VAULT};
pub use limits::{Clock, Limits, ManualClock, Rate, TrustedProxies, system_clock};
pub use logging::{LogFormat, install_panic_hook, log_subscriber, scrub};
pub use maintenance::{
    CleanUp, MAINTENANCE_EVERY, TOMBSTONE_HORIZON_DAYS, clean_up, spawn_maintenance,
};
pub use push::{
    APPROVAL_NOTICE, BACKGROUND_PAYLOAD, FCM_APPROVAL_DATA, FCM_SCOPE, FCM_SYNC_DATA, FcmConfig,
    PUSH_EVERY, Push, PushConfig,
};
pub use reports::{MAX_REPORT_BYTES, REPORT_RETENTION_DAYS, check_report, export_reports};
pub use routes::{MAX_BODY_BYTES, app};
pub use session::{ACCESS_TTL_SECS, REFRESH_TTL_SECS};
pub use vaults::MAX_VAULTS_PER_ACCOUNT;
