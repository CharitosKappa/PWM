use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("configuration: {0}")]
    Config(&'static str),
    #[error("database: {0}")]
    Database(#[from] sqlx::Error),
    #[error("database schema: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("network: {0}")]
    Io(#[from] std::io::Error),
}
