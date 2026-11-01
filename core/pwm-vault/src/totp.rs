//! TOTP (RFC 6238) codes from a login's stored value (decision D23): an
//! `otpauth://totp` URI, a bare base32 secret, or Steam Guard.

use std::fmt;

use pwm_crypto::{OtpAlgorithm, Secret, hotp_value};

use crate::VaultError;

const DEFAULT_DIGITS: u8 = 6;
const DEFAULT_PERIOD: u64 = 30;
const MAX_PERIOD: u64 = 3600;
const STEAM_ALPHABET: &[u8; 26] = b"23456789BCDFGHJKMNPQRTVWXY";
const STEAM_LEN: usize = 5;

/// What a code looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotpKind {
    /// 6-8 decimal digits.
    Digits(u8),
    /// 5 characters from Steam's alphabet.
    Steam,
}

/// A parsed TOTP configuration. The secret is zeroized on drop.
pub struct Totp {
    secret: Secret<Vec<u8>>,
    algorithm: OtpAlgorithm,
    kind: TotpKind,
    period: u64,
}

/// The code for one moment.
#[derive(Debug)]
pub struct TotpCode {
    pub code: Secret<String>,
    /// Seconds until the code changes (1..=period).
    pub seconds_left: u64,
}

impl Totp {
    /// Parses a stored or pasted value. Counter-based (`otpauth://hotp`) and
    /// other schemes are [`VaultError::Unsupported`]; keep such values as they
    /// are.
    pub fn parse(input: &str) -> Result<Self, VaultError> {
        let input = input.trim();
        if let Some(rest) = strip_prefix_ci(input, "steam://") {
            return Ok(Self {
                secret: decode_base32(rest)?,
                algorithm: OtpAlgorithm::Sha1,
                kind: TotpKind::Steam,
                period: DEFAULT_PERIOD,
            });
        }
        if let Some(rest) = strip_prefix_ci(input, "otpauth://") {
            return parse_otpauth(rest);
        }
        if input.contains("://") {
            return Err(VaultError::Unsupported("unknown one-time code scheme"));
        }
        Ok(Self {
            secret: decode_base32(input)?,
            algorithm: OtpAlgorithm::Sha1,
            kind: TotpKind::Digits(DEFAULT_DIGITS),
            period: DEFAULT_PERIOD,
        })
    }

    pub fn algorithm(&self) -> OtpAlgorithm {
        self.algorithm
    }

    pub fn kind(&self) -> TotpKind {
        self.kind
    }

    /// Seconds each code is valid for.
    pub fn period(&self) -> u64 {
        self.period
    }

    /// The code at `unix_secs` (seconds since the Unix epoch, from the
    /// caller's clock).
    pub fn code(&self, unix_secs: u64) -> TotpCode {
        let mut value = hotp_value(
            self.algorithm,
            self.secret.expose_secret(),
            unix_secs / self.period,
        );
        let code = Secret::<String>::new_with(|s| match self.kind {
            TotpKind::Digits(digits) => {
                let digits = usize::from(digits);
                s.reserve_exact(digits);
                let n = value % 10u32.pow(digits as u32);
                for i in (0..digits).rev() {
                    let d = n / 10u32.pow(i as u32) % 10;
                    s.push(char::from(b'0' + d as u8));
                }
            }
            TotpKind::Steam => {
                s.reserve_exact(STEAM_LEN);
                for _ in 0..STEAM_LEN {
                    s.push(char::from(STEAM_ALPHABET[(value % 26) as usize]));
                    value /= 26;
                }
            }
        });
        TotpCode {
            code,
            seconds_left: self.period - unix_secs % self.period,
        }
    }
}

impl fmt::Debug for Totp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Totp")
            .field("algorithm", &self.algorithm)
            .field("kind", &self.kind)
            .field("period", &self.period)
            .finish_non_exhaustive()
    }
}

/// The value to store for what the user entered: URIs (`otpauth://`,
/// `steam://`) unchanged so unknown parameters survive, a bare secret as
/// `otpauth://totp/<title>?secret=<SECRET>`. Invalid input is refused.
pub fn normalize_totp(input: &str, title: &str) -> Result<Secret<String>, VaultError> {
    let input = input.trim();
    if strip_prefix_ci(input, "otpauth://").is_some()
        || strip_prefix_ci(input, "steam://").is_some()
    {
        match Totp::parse(input) {
            Ok(_) | Err(VaultError::Unsupported(_)) => return Ok(Secret::new(input.to_owned())),
            Err(e) => return Err(e),
        }
    }
    let secret = decode_base32(input)?;
    Ok(Secret::<String>::new_with(|s| {
        s.push_str("otpauth://totp/");
        percent_encode_into(title, s);
        s.push_str("?secret=");
        encode_base32_into(secret.expose_secret(), s);
    }))
}

fn parse_otpauth(rest: &str) -> Result<Totp, VaultError> {
    let (kind, rest) = rest.split_once('/').unwrap_or((rest, ""));
    if !kind.eq_ignore_ascii_case("totp") {
        return Err(VaultError::Unsupported(
            "only time-based (totp) codes are supported",
        ));
    }
    let query = rest.split_once('?').map_or("", |(_, q)| q);
    let mut secret = None;
    let mut algorithm = OtpAlgorithm::Sha1;
    let mut digits = DEFAULT_DIGITS;
    let mut period = DEFAULT_PERIOD;
    let mut steam = false;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value)?;
        let value = value.expose_secret();
        match key.to_ascii_lowercase().as_str() {
            "secret" => secret = Some(decode_base32(value)?),
            "algorithm" => {
                algorithm = match value.to_ascii_uppercase().as_str() {
                    "SHA1" => OtpAlgorithm::Sha1,
                    "SHA256" => OtpAlgorithm::Sha256,
                    "SHA512" => OtpAlgorithm::Sha512,
                    _ => {
                        return Err(VaultError::Invalid(
                            "TOTP algorithm must be SHA1, SHA256 or SHA512",
                        ));
                    }
                }
            }
            "digits" => {
                digits = value
                    .parse()
                    .ok()
                    .filter(|d| (6..=8).contains(d))
                    .ok_or(VaultError::Invalid("TOTP digits must be 6-8"))?;
            }
            "period" => {
                period = value
                    .parse()
                    .ok()
                    .filter(|p| (1..=MAX_PERIOD).contains(p))
                    .ok_or(VaultError::Invalid("TOTP period must be 1-3600 seconds"))?;
            }
            "encoder" => steam = value.eq_ignore_ascii_case("steam"),
            _ => {}
        }
    }
    let secret = secret.ok_or(VaultError::Invalid("TOTP URI has no secret"))?;
    Ok(if steam {
        Totp {
            secret,
            algorithm: OtpAlgorithm::Sha1,
            kind: TotpKind::Steam,
            period: DEFAULT_PERIOD,
        }
    } else {
        Totp {
            secret,
            algorithm,
            kind: TotpKind::Digits(digits),
            period,
        }
    })
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// RFC 4648 base32, case-insensitive; spaces and `=` padding are ignored.
fn decode_base32(input: &str) -> Result<Secret<Vec<u8>>, VaultError> {
    const BAD: VaultError = VaultError::Invalid("TOTP secret is not valid base32");
    let mut ok = true;
    let mut chars = 0usize;
    let secret = Secret::<Vec<u8>>::new_with(|out| {
        out.reserve_exact(input.len() * 5 / 8 + 1);
        let (mut buffer, mut bits) = (0u32, 0u32);
        for c in input.bytes().filter(|c| !matches!(c, b' ' | b'=')) {
            let v = match c.to_ascii_uppercase() {
                c @ b'A'..=b'Z' => c - b'A',
                c @ b'2'..=b'7' => c - b'2' + 26,
                _ => {
                    ok = false;
                    return;
                }
            };
            chars += 1;
            buffer = (buffer << 5) | u32::from(v);
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
    });
    if !ok || chars == 0 || secret.expose_secret().is_empty() {
        return Err(BAD);
    }
    Ok(secret)
}

fn encode_base32_into(bytes: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
}

fn percent_decode(s: &str) -> Result<Secret<String>, VaultError> {
    const BAD: VaultError = VaultError::Invalid("TOTP URI has a bad percent-escape");
    let mut bytes = Secret::<Vec<u8>>::new_with(|out| out.reserve_exact(s.len()));
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        let byte = match b {
            b'%' => {
                let hi = iter
                    .next()
                    .and_then(|c| (c as char).to_digit(16))
                    .ok_or(BAD)?;
                let lo = iter
                    .next()
                    .and_then(|c| (c as char).to_digit(16))
                    .ok_or(BAD)?;
                (hi * 16 + lo) as u8
            }
            b'+' => b' ',
            b => b,
        };
        bytes.expose_secret_mut().push(byte);
    }
    let text = std::str::from_utf8(bytes.expose_secret()).map_err(|_| BAD)?;
    Ok(Secret::new(text.to_owned()))
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
fn percent_encode_into(s: &str, out: &mut String) {
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b >> 4)]));
            out.push(char::from(b"0123456789ABCDEF"[usize::from(b & 0x0F)]));
        }
    }
}
