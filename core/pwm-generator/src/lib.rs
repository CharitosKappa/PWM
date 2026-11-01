//! Password and passphrase generator with strength estimation (decision D21).
#![forbid(unsafe_code)]

mod error;
mod passphrase;
mod password;
mod strength;

pub use error::GeneratorError;
pub use passphrase::{PassphraseOptions, generate_passphrase, wordlist};
pub use password::{PasswordOptions, SYMBOLS, generate_password};
pub use strength::{MIN_SIGNUP_SCORE, Strength, strength};

use pwm_crypto::{Secret, random_index};

/// A generated password or passphrase.
#[derive(Debug)]
pub struct Generated {
    pub value: Secret<String>,
    /// Exact entropy of the generator's output distribution, in bits.
    pub entropy_bits: f64,
}

/// [`random_index`] over a choice set, which is always far smaller than
/// `u32::MAX` here.
fn pick(len: usize) -> Result<usize, GeneratorError> {
    let bound = u32::try_from(len).expect("choice set fits in u32");
    Ok(random_index(bound)? as usize)
}
