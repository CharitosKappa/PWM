use pwm_crypto::CryptoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GeneratorError {
    #[error("invalid generator options: {0}")]
    InvalidOptions(&'static str),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}
