//! The desktop app for Windows and Linux (D3, D66): a Tauri backend that
//! answers the shared screens (`@pwm/ui`) over one command.
#![deny(unsafe_code)]

mod app;
pub mod backend;
pub mod clipboard;
pub mod http;
mod items;
pub mod keys;
pub mod lock;
mod manage;
pub mod protocol;
pub mod settings;
mod system;

pub use app::run;
