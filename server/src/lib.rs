//! PWM sync server (D25, D26, D27): an axum API over Postgres. It stores
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
mod routes;
mod session;
mod vaults;

pub use api::AppState;
pub use config::{Config, DEFAULT_LISTEN, generate_server_secrets};
pub use db::{MIGRATOR, connect, migrate};
pub use email::normalize_email;
pub use error::ServerError;
pub use routes::app;
pub use session::{ACCESS_TTL_SECS, REFRESH_TTL_SECS};
