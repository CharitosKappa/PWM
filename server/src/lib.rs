//! PWM sync server (D25–D27, D29, D30): an axum API over Postgres. It stores
//! ciphertext, sizes, ids and times only, plus the account email.
#![forbid(unsafe_code)]

mod account;
mod api;
mod auth;
mod config;
mod db;
mod email;
mod error;
mod items;
mod limits;
mod logging;
mod reports;
mod routes;
mod session;
mod vaults;

pub use api::AppState;
pub use config::{Config, DEFAULT_LISTEN, generate_server_secrets};
pub use db::{MIGRATOR, connect, healthcheck, mark_restored, migrate};
pub use email::normalize_email;
pub use error::ServerError;
pub use items::{MAX_ITEM_BLOB_BYTES, MAX_ITEMS_PER_VAULT};
pub use limits::{Clock, Limits, ManualClock, Rate, TrustedProxies, system_clock};
pub use logging::{LogFormat, install_panic_hook, log_subscriber, scrub};
pub use reports::{MAX_REPORT_BYTES, REPORT_RETENTION_DAYS, check_report, export_reports};
pub use routes::{MAX_BODY_BYTES, app};
pub use session::{ACCESS_TTL_SECS, REFRESH_TTL_SECS};
pub use vaults::MAX_VAULTS_PER_ACCOUNT;
