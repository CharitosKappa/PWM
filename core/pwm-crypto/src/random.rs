//! Uniform random choices from the OS CSPRNG (decision D21).

use crate::CryptoError;

/// A uniform random integer in `0..bound`, by rejection sampling so no value
/// is more likely than another.
///
/// # Panics
/// If `bound` is 0.
pub fn random_index(bound: u32) -> Result<u32, CryptoError> {
    assert!(bound > 0, "bound must be positive");
    // Accept only draws below the largest multiple of `bound` in 0..2^32.
    let rejected = (u32::MAX - bound + 1) % bound; // 2^32 mod bound
    let limit = u32::MAX - rejected;
    loop {
        let mut bytes = [0u8; 4];
        getrandom::fill(&mut bytes).map_err(|_| CryptoError::Rng)?;
        let draw = u32::from_le_bytes(bytes);
        if draw <= limit {
            return Ok(draw % bound);
        }
    }
}
