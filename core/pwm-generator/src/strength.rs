use zxcvbn::zxcvbn;

/// Lowest score accepted for a master password at signup.
pub const MIN_SIGNUP_SCORE: u8 = 3;

/// Only this many characters are scored, so a huge paste cannot stall the UI.
const MAX_SCORED_CHARS: usize = 100;

/// A zxcvbn estimate. Holds no part of the password.
#[derive(Debug, Clone, PartialEq)]
pub struct Strength {
    /// 0 (too guessable) to 4 (very unguessable).
    pub score: u8,
    pub guesses_log10: f64,
    /// Time to crack offline against a slow hash (10^4 guesses/s), such as
    /// "3 hours" or "centuries".
    pub crack_time: String,
    pub warning: Option<String>,
    pub suggestions: Vec<String>,
}

/// Estimates how guessable `password` is. `context` holds words the user is
/// likely to reuse (email, name, item title), which then score as weak.
pub fn strength(password: &str, context: &[&str]) -> Strength {
    let end = password
        .char_indices()
        .nth(MAX_SCORED_CHARS)
        .map_or(password.len(), |(i, _)| i);
    let estimate = zxcvbn(&password[..end], context);
    let feedback = estimate.feedback();
    Strength {
        score: u8::from(estimate.score()),
        guesses_log10: estimate.guesses_log10(),
        crack_time: estimate
            .crack_times()
            .offline_slow_hashing_1e4_per_second()
            .to_string(),
        warning: feedback.and_then(|f| f.warning()).map(|w| w.to_string()),
        suggestions: feedback
            .map(|f| f.suggestions().iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
    }
}
