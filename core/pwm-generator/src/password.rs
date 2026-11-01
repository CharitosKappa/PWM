use pwm_crypto::Secret;

use crate::{Generated, GeneratorError, pick};

/// Symbols most sites accept.
pub const SYMBOLS: &str = "!#$%&*+-=?@^_";

const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const LOOKALIKES: &str = "0Oo1lI";

const MIN_LENGTH: usize = 8;
const MAX_LENGTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordOptions {
    /// Number of characters, 8-128.
    pub length: usize,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
    /// Leave out `0 O o 1 l I`.
    pub avoid_lookalikes: bool,
}

impl Default for PasswordOptions {
    fn default() -> Self {
        Self {
            length: 20,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            avoid_lookalikes: false,
        }
    }
}

/// A random password containing every selected character class at least once.
pub fn generate_password(opts: &PasswordOptions) -> Result<Generated, GeneratorError> {
    if !(MIN_LENGTH..=MAX_LENGTH).contains(&opts.length) {
        return Err(GeneratorError::InvalidOptions("length must be 8-128"));
    }
    let classes = classes(opts);
    if classes.is_empty() {
        return Err(GeneratorError::InvalidOptions(
            "select at least one character class",
        ));
    }
    let alphabet: Vec<char> = classes.iter().flatten().copied().collect();
    loop {
        // A candidate missing a class is discarded (and zeroized) and redrawn,
        // which keeps the result uniform over all valid passwords.
        let mut result = Ok(());
        let candidate = Secret::<String>::new_with(|buf| {
            buf.reserve_exact(opts.length);
            for _ in 0..opts.length {
                match pick(alphabet.len()) {
                    Ok(i) => buf.push(alphabet[i]),
                    Err(e) => {
                        result = Err(e);
                        return;
                    }
                }
            }
        });
        result?;
        let value = candidate.expose_secret();
        if classes
            .iter()
            .all(|class| value.chars().any(|c| class.contains(&c)))
        {
            return Ok(Generated {
                entropy_bits: entropy_bits(&classes, opts.length),
                value: candidate,
            });
        }
    }
}

fn classes(opts: &PasswordOptions) -> Vec<Vec<char>> {
    [
        (opts.lowercase, LOWERCASE),
        (opts.uppercase, UPPERCASE),
        (opts.digits, DIGITS),
        (opts.symbols, SYMBOLS),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, chars)| {
        chars
            .chars()
            .filter(|c| !(opts.avoid_lookalikes && LOOKALIKES.contains(*c)))
            .collect()
    })
    .collect()
}

/// log2 of the number of length-`len` strings over the union of `classes`
/// that use every class, by inclusion-exclusion over the classes left out:
/// `A^len * sum over subsets S of (-1)^|S| * (1 - |S| / A)^len`.
fn entropy_bits(classes: &[Vec<char>], len: usize) -> f64 {
    let a = classes.iter().map(Vec::len).sum::<usize>() as f64;
    let len = i32::try_from(len).expect("length is at most 128");
    let mut fraction = 0.0;
    for subset in 0u32..(1 << classes.len()) {
        let left_out: usize = (0..classes.len())
            .filter(|i| subset & (1 << i) != 0)
            .map(|i| classes[i].len())
            .sum();
        let term = (1.0 - left_out as f64 / a).powi(len);
        fraction += if subset.count_ones() % 2 == 0 {
            term
        } else {
            -term
        };
    }
    f64::from(len) * a.log2() + fraction.log2()
}
