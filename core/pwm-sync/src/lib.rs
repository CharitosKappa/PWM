//! Client sync engine (decision D28): offline queue, retries with backoff
//! and conflict resolution against the D27 server API. It does no I/O: the
//! platform sends each [`HttpRequest`] and hands back the [`HttpResponse`].
#![forbid(unsafe_code)]

mod apply;
mod backoff;
mod devices;
mod engine;
mod error;
mod http;
mod tokens;
mod wire;

pub use backoff::{BACKOFF_BASE_MS, BACKOFF_CAP_MS, backoff_delay_ms};
pub use devices::{DeviceRecord, DeviceTask};
pub use engine::{
    DEFAULT_PAGE_LIMIT, MAX_CONFLICT_RETRIES, Step, SyncEngine, SyncEvent, SyncOutcome, SyncReport,
};
pub use error::SyncError;
pub use http::{HttpRequest, HttpResponse, Method};
pub use tokens::Tokens;
