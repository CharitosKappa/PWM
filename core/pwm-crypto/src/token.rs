//! Session tokens (decision D26): 32 random bytes as base64url with a kind
//! prefix, so secret scanners can spot leaked ones. The server stores only
//! [`token_hash`], never the token.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::account::random_32;
use crate::{CryptoError, Secret};

/// Base64url length of 32 bytes without padding.
const ENCODED_LEN: usize = 43;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Short-lived, sent as `Authorization: Bearer`.
    Access,
    /// Long-lived, exchanged for a new token pair.
    Refresh,
    /// A new device's handle on its approval request (D46), for 10 minutes.
    Approval,
}

impl TokenKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Access => "pwma_",
            Self::Refresh => "pwmr_",
            Self::Approval => "pwmp_",
        }
    }
}

/// A new random token of this kind.
pub fn generate_token(kind: TokenKind) -> Result<Secret<String>, CryptoError> {
    let bytes = random_32()?;
    Ok(Secret::<String>::new_with(|s| {
        s.reserve_exact(kind.prefix().len() + ENCODED_LEN);
        s.push_str(kind.prefix());
        URL_SAFE_NO_PAD.encode_string(bytes.expose_secret(), s);
    }))
}

/// SHA-256 of a well-formed token of this kind, or `None` for anything else
/// (wrong prefix, length or encoding). Tokens are random, so a plain hash is
/// enough to keep a database leak from yielding usable tokens.
pub fn token_hash(kind: TokenKind, token: &str) -> Option<[u8; 32]> {
    let encoded = token.strip_prefix(kind.prefix())?;
    if encoded.len() != ENCODED_LEN {
        return None;
    }
    // Canonical base64url only, so one token has exactly one spelling.
    let mut raw = [0u8; 33];
    let decoded = URL_SAFE_NO_PAD.decode_slice(encoded, &mut raw);
    raw.zeroize();
    if decoded != Ok(32) {
        return None;
    }
    Some(Sha256::digest(token.as_bytes()).into())
}
