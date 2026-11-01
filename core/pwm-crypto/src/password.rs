//! Master password preparation (decision D16, RFC 8265 OpaqueString rules).

use unicode_normalization::UnicodeNormalization;

use crate::{CryptoError, Secret};

/// Non-ASCII characters in Unicode category Zs (space separators).
const NON_ASCII_SPACES: &[char] = &[
    '\u{a0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}',
    '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200a}', '\u{202f}', '\u{205f}', '\u{3000}',
];

/// Maps non-ASCII spaces to U+0020 and normalizes to NFC, so the same
/// password gives the same KDF input on every platform. Keeps case and never
/// trims. Rejects empty passwords and control characters.
pub fn normalize_password(password: &str) -> Result<Secret<String>, CryptoError> {
    let mapped = Secret::<String>::new_with(|s| {
        s.reserve_exact(password.len());
        s.extend(password.chars().map(|c| {
            if NON_ASCII_SPACES.contains(&c) {
                ' '
            } else {
                c
            }
        }));
    });
    let normalized = Secret::<String>::new_with(|s| {
        s.reserve_exact(mapped.expose_secret().len());
        s.extend(mapped.expose_secret().nfc());
    });
    let text = normalized.expose_secret();
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(CryptoError::PasswordRejected);
    }
    Ok(normalized)
}
