//! The Emergency Kit (decision D42 §5): a one-page PDF with what a new device
//! needs to sign in, and the QR code on it that fills in the sign-in form.
//!
//! The master password is never on the kit or in its QR code; the page has a
//! blank line to write it by hand.
#![forbid(unsafe_code)]

mod error;
mod layout;
mod pdf;
mod qr;

pub use error::KitError;
pub use qr::{KitQr, parse_kit_qr};

use pwm_crypto::{Secret, SecretKey};

/// The product name on the kit, a placeholder until D1.
pub const PRODUCT_NAME: &str = "PWM";

/// Paper size, chosen by the platform from the user's locale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    A4,
    Letter,
}

impl Paper {
    /// Width and height in points.
    fn size(self) -> (u32, u32) {
        match self {
            Paper::A4 => (595, 842),
            Paper::Letter => (612, 792),
        }
    }
}

/// What goes on the kit.
#[derive(Debug)]
pub struct Kit<'a> {
    pub email: &'a str,
    pub secret_key: &'a SecretKey,
    /// A self-hosted server's address; `None` for the hosted service.
    pub server: Option<&'a str>,
    pub account_id: &'a str,
    /// The creation date as `YYYY-MM-DD`, in the user's time zone.
    pub created: &'a str,
}

impl Kit<'_> {
    /// The QR code's text: `pwm-kit:1?email=…&secret-key=…[&server=…]`.
    pub fn qr_payload(&self) -> Result<Secret<String>, KitError> {
        check_email(self.email)?;
        if let Some(server) = self.server {
            check_server(server)?;
        }
        Ok(qr::payload(self.email, self.secret_key, self.server))
    }
}

/// The kit as PDF bytes. One input always gives the same bytes.
pub fn kit_pdf(kit: &Kit<'_>, paper: Paper) -> Result<Secret<Vec<u8>>, KitError> {
    check_email(kit.email)?;
    if let Some(server) = kit.server {
        check_server(server)?;
    }
    check_account_id(kit.account_id)?;
    check_date(kit.created)?;
    let payload = kit.qr_payload()?;
    let modules = qr::modules(payload.expose_secret())?;
    let content = layout::page(kit, paper, &modules);
    Ok(pdf::document(paper.size(), &content))
}

/// The longest email the server accepts (RFC 5321 path limit).
const MAX_EMAIL: usize = 254;
/// Long enough for any real server address, short enough to fit on the page.
const MAX_SERVER: usize = 200;

fn check_email(email: &str) -> Result<(), KitError> {
    let ok = !email.is_empty()
        && email.len() <= MAX_EMAIL
        && email.contains('@')
        && !email.chars().any(|c| c.is_control() || c.is_whitespace());
    if ok {
        Ok(())
    } else {
        Err(KitError::InvalidEmail)
    }
}

fn check_server(server: &str) -> Result<(), KitError> {
    let host = if let Some(rest) = server.strip_prefix("https://") {
        rest
    } else if let Some(rest) = server.strip_prefix("http://") {
        // Plain HTTP only for a server on this machine, as in the extension.
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
        if !matches!(name, "localhost" | "127.0.0.1" | "[::1]") {
            return Err(KitError::InvalidServer);
        }
        rest
    } else {
        return Err(KitError::InvalidServer);
    };
    let ok = !host.is_empty()
        && server.len() <= MAX_SERVER
        && !server.chars().any(|c| c.is_control() || c.is_whitespace());
    if ok {
        Ok(())
    } else {
        Err(KitError::InvalidServer)
    }
}

/// A UUID in its 36-character text form.
fn check_account_id(id: &str) -> Result<(), KitError> {
    let ok = id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    if ok {
        Ok(())
    } else {
        Err(KitError::InvalidAccountId)
    }
}

fn check_date(date: &str) -> Result<(), KitError> {
    let b = date.as_bytes();
    let digits = |r: core::ops::Range<usize>| -> Option<u32> {
        let s = date.get(r)?;
        if s.bytes().all(|c| c.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && digits(0..4).is_some()
        && digits(5..7).is_some_and(|m| (1..=12).contains(&m))
        && digits(8..10).is_some_and(|d| (1..=31).contains(&d));
    if ok {
        Ok(())
    } else {
        Err(KitError::InvalidDate)
    }
}
