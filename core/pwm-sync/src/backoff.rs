//! Retry delays (D28): exponential backoff with full jitter.

/// The longest possible wait after the first failure.
pub const BACKOFF_BASE_MS: u64 = 1_000;
/// The longest possible wait after many failures (5 minutes).
pub const BACKOFF_CAP_MS: u64 = 300_000;
/// `Retry-After` values above this (1 hour) are clamped.
const RETRY_AFTER_MAX_MS: u64 = 3_600_000;

/// The wait after `failures` earlier failed runs: a uniform `fraction`
/// (0 to 1) of `min(cap, base * 2^failures)`, but never less than the
/// server's `Retry-After`.
pub fn backoff_delay_ms(failures: u32, fraction: f64, retry_after_ms: Option<u64>) -> u64 {
    let window = 1u64
        .checked_shl(failures)
        .map_or(u64::MAX, |m| BACKOFF_BASE_MS.saturating_mul(m))
        .min(BACKOFF_CAP_MS);
    let jitter = (window as f64 * fraction.clamp(0.0, 1.0)) as u64;
    jitter.max(retry_after_ms.unwrap_or(0).min(RETRY_AFTER_MAX_MS))
}

/// A uniform fraction in [0, 1) from the OS RNG; the largest window if the
/// RNG fails, which only delays a retry.
pub(crate) fn random_fraction() -> f64 {
    const STEPS: u32 = 1_000_000;
    pwm_crypto::random_index(STEPS).map_or(1.0, |n| f64::from(n) / f64::from(STEPS))
}

/// `Retry-After` in whole seconds, as milliseconds.
pub(crate) fn parse_retry_after(value: Option<&str>) -> Option<u64> {
    value?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|s| s.saturating_mul(1_000))
}
