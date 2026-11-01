//! Password and passphrase generator, strength estimate (decision D21).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use pwm_generator::{
    GeneratorError, MIN_SIGNUP_SCORE, PassphraseOptions, PasswordOptions, SYMBOLS,
    generate_passphrase, generate_password, strength, wordlist,
};

const LOOKALIKES: &str = "0Oo1lI";

fn password(opts: &PasswordOptions) -> String {
    generate_password(opts)
        .unwrap()
        .value
        .expose_secret()
        .clone()
}

fn only(lowercase: bool, uppercase: bool, digits: bool, symbols: bool) -> PasswordOptions {
    PasswordOptions {
        lowercase,
        uppercase,
        digits,
        symbols,
        ..PasswordOptions::default()
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// --- Passwords ---

#[test]
fn password_defaults() {
    let opts = PasswordOptions::default();
    assert_eq!(opts.length, 20);
    assert!(opts.lowercase && opts.uppercase && opts.digits && opts.symbols);
    assert!(!opts.avoid_lookalikes);
    assert_eq!(SYMBOLS, "!#$%&*+-=?@^_");

    let p = password(&opts);
    assert_eq!(p.chars().count(), 20);
    assert!(
        p.chars()
            .all(|c| c.is_ascii_alphanumeric() || SYMBOLS.contains(c))
    );
}

#[test]
fn every_selected_class_appears() {
    let opts = PasswordOptions {
        length: 8,
        ..PasswordOptions::default()
    };
    for _ in 0..500 {
        let p = password(&opts);
        assert!(p.chars().any(|c| c.is_ascii_lowercase()), "{p}");
        assert!(p.chars().any(|c| c.is_ascii_uppercase()), "{p}");
        assert!(p.chars().any(|c| c.is_ascii_digit()), "{p}");
        assert!(p.chars().any(|c| SYMBOLS.contains(c)), "{p}");
    }
}

#[test]
fn unselected_classes_never_appear() {
    for _ in 0..200 {
        assert!(
            password(&only(false, false, true, false))
                .chars()
                .all(|c| c.is_ascii_digit())
        );
        assert!(
            password(&only(true, true, false, false))
                .chars()
                .all(|c| c.is_ascii_alphabetic())
        );
    }
}

#[test]
fn every_character_can_appear() {
    let mut seen = HashSet::new();
    for _ in 0..1_000 {
        seen.extend(password(&PasswordOptions::default()).chars());
    }
    assert_eq!(seen.len(), 26 + 26 + 10 + SYMBOLS.len());
}

#[test]
fn lookalikes_can_be_left_out() {
    let opts = PasswordOptions {
        avoid_lookalikes: true,
        ..PasswordOptions::default()
    };
    let mut seen = HashSet::new();
    for _ in 0..1_000 {
        let p = password(&opts);
        assert!(!p.chars().any(|c| LOOKALIKES.contains(c)), "{p}");
        seen.extend(p.chars());
    }
    assert_eq!(seen.len(), 24 + 24 + 8 + SYMBOLS.len());
}

#[test]
fn passwords_do_not_repeat() {
    let mut seen = HashSet::new();
    for _ in 0..1_000 {
        assert!(seen.insert(password(&PasswordOptions::default())));
    }
}

#[test]
fn password_options_are_checked() {
    for length in [8, 128] {
        let opts = PasswordOptions {
            length,
            ..PasswordOptions::default()
        };
        assert_eq!(password(&opts).len(), length);
    }
    for length in [0, 7, 129] {
        let opts = PasswordOptions {
            length,
            ..PasswordOptions::default()
        };
        assert!(matches!(
            generate_password(&opts),
            Err(GeneratorError::InvalidOptions(_))
        ));
    }
    assert!(matches!(
        generate_password(&only(false, false, false, false)),
        Err(GeneratorError::InvalidOptions(_))
    ));
}

#[test]
fn password_entropy_counts_only_valid_passwords() {
    // One class: every string is valid.
    let digits = PasswordOptions {
        length: 10,
        ..only(false, false, true, false)
    };
    assert!(close(
        generate_password(&digits).unwrap().entropy_bits,
        10.0 * 10f64.log2()
    ));

    // Default: 75 characters, all four classes required.
    let bits = generate_password(&PasswordOptions::default())
        .unwrap()
        .entropy_bits;
    assert!(
        bits < 20.0 * 75f64.log2() && bits > 20.0 * 75f64.log2() - 1.0,
        "{bits}"
    );
}

#[test]
fn password_entropy_matches_a_brute_force_count() {
    // Length 8 over lowercase + digits, both required:
    // 36^8 - 26^8 - 10^8 valid strings.
    let opts = PasswordOptions {
        length: 8,
        ..only(true, false, true, false)
    };
    let valid = 36f64.powi(8) - 26f64.powi(8) - 10f64.powi(8);
    let bits = generate_password(&opts).unwrap().entropy_bits;
    assert!((bits - valid.log2()).abs() < 1e-9, "{bits}");
}

// --- Passphrases ---

#[test]
fn the_wordlist_is_the_eff_large_list() {
    let words = wordlist();
    assert_eq!(words.len(), 7_776);
    assert_eq!(words[0], "abacus");
    assert_eq!(words[7_775], "zoom");
    assert_eq!(words.iter().collect::<HashSet<_>>().len(), 7_776);
    assert!(words.iter().all(
        |w| (3..=9).contains(&w.len()) && w.chars().all(|c| c.is_ascii_lowercase() || c == '-')
    ));
}

#[test]
fn passphrase_defaults() {
    let opts = PassphraseOptions::default();
    assert_eq!(opts.words, 5);
    assert_eq!(opts.separator, "-");
    assert!(!opts.capitalize && !opts.add_digit);

    let g = generate_passphrase(&opts).unwrap();
    assert!(close(g.entropy_bits, 5.0 * 7_776f64.log2()));
    assert!(g.value.expose_secret().contains('-'));
}

#[test]
fn passphrase_words_come_from_the_list() {
    let list: HashSet<&str> = wordlist().iter().copied().collect();
    let opts = PassphraseOptions {
        words: 6,
        separator: " ".into(),
        ..PassphraseOptions::default()
    };
    for _ in 0..200 {
        let g = generate_passphrase(&opts).unwrap();
        let words: Vec<&str> = g.value.expose_secret().split(' ').collect();
        assert_eq!(words.len(), 6);
        assert!(words.iter().all(|w| list.contains(w)), "{words:?}");
    }
}

#[test]
fn passphrase_capitalize_and_digit() {
    let opts = PassphraseOptions {
        separator: ".".into(),
        capitalize: true,
        add_digit: true,
        ..PassphraseOptions::default()
    };
    for _ in 0..200 {
        let g = generate_passphrase(&opts).unwrap();
        let phrase = g.value.expose_secret();
        assert!(
            phrase
                .split('.')
                .all(|w| w.starts_with(|c: char| c.is_ascii_uppercase()))
        );
        assert_eq!(phrase.chars().filter(char::is_ascii_digit).count(), 1);
        let digit_word = phrase
            .split('.')
            .find(|w| w.ends_with(|c: char| c.is_ascii_digit()));
        assert!(digit_word.is_some(), "{phrase}");
    }
    let bits = generate_passphrase(&opts).unwrap().entropy_bits;
    assert!(close(bits, 5.0 * 7_776f64.log2() + 50f64.log2()));
}

#[test]
fn passphrase_options_are_checked() {
    for words in [3, 20] {
        let opts = PassphraseOptions {
            words,
            separator: " ".into(),
            ..PassphraseOptions::default()
        };
        let g = generate_passphrase(&opts).unwrap();
        assert_eq!(g.value.expose_secret().split(' ').count(), words);
    }
    let bad_words = [2, 21].map(|words| PassphraseOptions {
        words,
        ..PassphraseOptions::default()
    });
    let bad_separators = ["", "____", "a", "\n"].map(|s| PassphraseOptions {
        separator: s.into(),
        ..PassphraseOptions::default()
    });
    for opts in bad_words.iter().chain(&bad_separators) {
        assert!(matches!(
            generate_passphrase(opts),
            Err(GeneratorError::InvalidOptions(_))
        ));
    }
}

// --- Strength ---

#[test]
fn common_passwords_score_low() {
    assert_eq!(MIN_SIGNUP_SCORE, 3);
    for weak in ["password", "123456", "qwerty", "iloveyou1"] {
        let s = strength(weak, &[]);
        assert!(s.score < 2, "{weak}: {}", s.score);
        assert!(s.warning.is_some() || !s.suggestions.is_empty());
    }
    assert_eq!(strength("", &[]).score, 0);
}

#[test]
fn generated_values_score_high() {
    let p = generate_password(&PasswordOptions::default()).unwrap();
    assert_eq!(strength(p.value.expose_secret(), &[]).score, 4);
    let g = generate_passphrase(&PassphraseOptions::default()).unwrap();
    assert!(strength(g.value.expose_secret(), &[]).score >= MIN_SIGNUP_SCORE);
}

#[test]
fn context_lowers_the_estimate() {
    let alone = strength("Kappatos-Fashion", &[]);
    let with = strength("Kappatos-Fashion", &["kappatos", "fashion"]);
    assert!(with.guesses_log10 < alone.guesses_log10);
    assert!(!with.crack_time.is_empty());
}

#[test]
fn very_long_input_is_scored_quickly() {
    let chunk = PasswordOptions {
        length: 128,
        ..PasswordOptions::default()
    };
    let long: String = (0..80).map(|_| password(&chunk)).collect();
    let start = Instant::now();
    let s = strength(&long, &[]);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(s.score, 4);
    // Only the first 100 characters count.
    assert!(close(
        s.guesses_log10,
        strength(&long[..100], &[]).guesses_log10
    ));
}

#[test]
fn debug_output_hides_generated_values() {
    let g = generate_password(&PasswordOptions::default()).unwrap();
    let debug = format!("{g:?}");
    assert!(!debug.contains(g.value.expose_secret().as_str()));
    assert!(debug.contains("REDACTED"));
}
