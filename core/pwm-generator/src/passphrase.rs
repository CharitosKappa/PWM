use std::sync::LazyLock;

use pwm_crypto::Secret;

use crate::{Generated, GeneratorError, pick};

/// EFF large wordlist, unmodified (`data/NOTICE.md`): `<dice>\t<word>` lines.
const EFF_LARGE: &str = include_str!("../data/eff_large_wordlist.txt");

static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    EFF_LARGE
        .lines()
        .map(|line| line.split_once('\t').expect("dice<TAB>word").1)
        .collect()
});

const MIN_WORDS: usize = 3;
const MAX_WORDS: usize = 20;
const MAX_SEPARATOR_CHARS: usize = 3;
/// Longest word in the list.
const MAX_WORD_LEN: usize = 9;

/// The 7,776 passphrase words.
pub fn wordlist() -> &'static [&'static str] {
    &WORDS
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassphraseOptions {
    /// Number of words, 3-20.
    pub words: usize,
    /// 1-3 characters, no letters, digits or control characters.
    pub separator: String,
    /// Capitalize the first letter of each word.
    pub capitalize: bool,
    /// Append one random digit to one random word.
    pub add_digit: bool,
}

impl Default for PassphraseOptions {
    fn default() -> Self {
        Self {
            words: 5,
            separator: "-".into(),
            capitalize: false,
            add_digit: false,
        }
    }
}

pub fn generate_passphrase(opts: &PassphraseOptions) -> Result<Generated, GeneratorError> {
    if !(MIN_WORDS..=MAX_WORDS).contains(&opts.words) {
        return Err(GeneratorError::InvalidOptions("words must be 3-20"));
    }
    let separator_chars = opts.separator.chars().count();
    if !(1..=MAX_SEPARATOR_CHARS).contains(&separator_chars)
        || opts
            .separator
            .chars()
            .any(|c| c.is_alphanumeric() || c.is_control())
    {
        return Err(GeneratorError::InvalidOptions(
            "separator must be 1-3 characters, no letters, digits or control characters",
        ));
    }
    let words = wordlist();
    let digit = if opts.add_digit {
        Some((pick(opts.words)?, pick(10)?))
    } else {
        None
    };

    let mut result = Ok(());
    let value = Secret::<String>::new_with(|buf| {
        buf.reserve_exact(opts.words * (MAX_WORD_LEN + opts.separator.len()) + 1);
        for n in 0..opts.words {
            let word = match pick(words.len()) {
                Ok(i) => words[i],
                Err(e) => {
                    result = Err(e);
                    return;
                }
            };
            if n > 0 {
                buf.push_str(&opts.separator);
            }
            let mut chars = word.chars();
            if opts.capitalize
                && let Some(first) = chars.next()
            {
                buf.push(first.to_ascii_uppercase());
            }
            buf.push_str(chars.as_str());
            if let Some((at, d)) = digit
                && at == n
            {
                buf.push(char::from(b'0' + d as u8));
            }
        }
    });
    result?;

    let mut entropy_bits = opts.words as f64 * (words.len() as f64).log2();
    if opts.add_digit {
        entropy_bits += (10.0 * opts.words as f64).log2();
    }
    Ok(Generated {
        value,
        entropy_bits,
    })
}
