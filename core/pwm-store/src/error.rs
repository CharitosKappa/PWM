use thiserror::Error;

/// Store errors. Messages never contain stored bytes.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The database was written by a newer app version; it is left untouched.
    #[error("cache schema v{found} is newer than supported v{supported}")]
    NewerSchema { found: u32, supported: u32 },
    /// The file is a database that does not belong to this app.
    #[error("not a cache database")]
    NotAStore,
    /// A stored value failed to decode.
    #[error("cache is corrupt")]
    Corrupt,
    #[error("invalid record: {0}")]
    Invalid(&'static str),
    /// The storage backend failed (I/O, locking, ...).
    #[error("storage error: {0}")]
    Backend(String),
}
