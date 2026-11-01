//! PWM sync server (D25): an axum API over Postgres. It stores ciphertext,
//! sizes, ids and times only, plus the account email.
#![forbid(unsafe_code)]

mod config;
mod db;
mod error;
mod routes;

pub use config::{Config, DEFAULT_LISTEN};
pub use db::{MIGRATOR, connect, migrate};
pub use error::ServerError;
pub use routes::app;
