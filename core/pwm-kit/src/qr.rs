//! The kit's QR code text, format 1: `pwm-kit:1?email=…&secret-key=…[&server=…]`,
//! each value percent-encoded as UTF-8 (RFC 3986). It is not a registered URL
//! scheme, so only the app's own scanner reads it (D42 §5).

use pwm_crypto::{Secret, SecretKey};
use qrcode::{Color, EcLevel, QrCode};
use zeroize::Zeroizing;

use crate::{KitError, check_email, check_server};

const SCHEME: &str = "pwm-kit:";
const VERSION: &str = "1";

/// A scanned kit: what it fills in on the sign-in screen.
#[derive(Debug)]
pub struct KitQr {
    pub email: String,
    pub secret_key: SecretKey,
    /// A self-hosted server's address; `None` for the hosted service.
    pub server: Option<String>,
}

pub(crate) fn payload(email: &str, secret_key: &SecretKey, server: Option<&str>) -> Secret<String> {
    let key = secret_key.to_text();
    Secret::<String>::new_with(|s| {
        // Room for the longest inputs, so the buffer never reallocates and
        // leaves a copy of the key behind.
        s.reserve_exact(SCHEME.len() + 64 + 3 * (crate::MAX_EMAIL + crate::MAX_SERVER));
        s.push_str(SCHEME);
        s.push_str(VERSION);
        s.push_str("?email=");
        encode(s, email);
        s.push_str("&secret-key=");
        encode(s, key.expose_secret());
        if let Some(server) = server {
            s.push_str("&server=");
            encode(s, server);
        }
    })
}

/// The QR code's modules, row by row (`true` is dark), and its width.
/// The encoder's own buffers hold the text briefly and are not wiped.
pub(crate) struct Modules {
    pub width: usize,
    pub dark: Vec<bool>,
}

pub(crate) fn modules(text: &str) -> Result<Modules, KitError> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)
        .map_err(|_| KitError::Qr)?;
    let width = code.width();
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == Color::Dark)
        .collect();
    Ok(Modules { width, dark })
}

/// Reads a scanned code. Unknown fields are ignored, so a later version-1 kit
/// with more fields still scans; a field given twice is refused.
pub fn parse_kit_qr(text: &str) -> Result<KitQr, KitError> {
    let rest = text.strip_prefix(SCHEME).ok_or(KitError::NotAKit)?;
    let (version, query) = rest.split_once('?').unwrap_or((rest, ""));
    if version != VERSION {
        return Err(
            if !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()) {
                KitError::UnsupportedVersion
            } else {
                KitError::NotAKit
            },
        );
    }

    let mut email = None;
    let mut key = None;
    let mut server = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').ok_or(KitError::NotAKit)?;
        let slot = match name {
            "email" => &mut email,
            "secret-key" => &mut key,
            "server" => &mut server,
            _ => continue,
        };
        if slot.replace(decode(value)?).is_some() {
            return Err(KitError::NotAKit);
        }
    }
    let (Some(email), Some(key)) = (email, key) else {
        return Err(KitError::NotAKit);
    };

    check_email(&email)?;
    let secret_key = SecretKey::parse(&key).map_err(|_| KitError::InvalidSecretKey)?;
    if let Some(server) = &server {
        check_server(server)?;
    }
    Ok(KitQr {
        email: email.to_string(),
        secret_key,
        server: server.map(|s| s.to_string()),
    })
}

fn encode(out: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in value.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0xF)]));
        }
    }
}

fn decode(value: &str) -> Result<Zeroizing<String>, KitError> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(value.len()));
    let mut input = value.bytes();
    while let Some(b) = input.next() {
        if b == b'%' {
            let hex = |c: Option<u8>| char::from(c.unwrap_or(b'x')).to_digit(16);
            let (Some(hi), Some(lo)) = (hex(input.next()), hex(input.next())) else {
                return Err(KitError::NotAKit);
            };
            bytes.push((hi * 16 + lo) as u8);
        } else {
            bytes.push(b);
        }
    }
    let text = core::str::from_utf8(&bytes).map_err(|_| KitError::NotAKit)?;
    Ok(Zeroizing::new(text.to_owned()))
}
