//! What the providers' clients share: one HTTPS client (HTTP/1.1 or 2),
//! form encoding, JWS payloads, RFC 3339 times and minor currency units.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

use super::BillingError;

const TIMEOUT: Duration = Duration::from_secs(15);
/// The most of a provider's answer read.
const MAX_ANSWER: usize = 1024 * 1024;

pub(crate) struct Http {
    client: Client<hyper_rustls::HttpsConnector<HttpConnector>, Full<Bytes>>,
}

impl Http {
    /// `None` without the system's root certificates.
    pub(crate) fn new() -> Option<Self> {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .ok()?
            .https_or_http()
            .enable_all_versions()
            .build();
        Some(Self {
            client: Client::builder(TokioExecutor::new()).build(connector),
        })
    }

    /// Sends one request: the status and the body, or `Unavailable` when
    /// the provider can't be reached in time.
    pub(crate) async fn send(
        &self,
        request: hyper::Request<Full<Bytes>>,
    ) -> Result<(u16, Bytes), BillingError> {
        let response = tokio::time::timeout(TIMEOUT, self.client.request(request))
            .await
            .map_err(|_| BillingError::Unavailable)?
            .map_err(|_| BillingError::Unavailable)?;
        let status = response.status().as_u16();
        let body = http_body_util::Limited::new(response.into_body(), MAX_ANSWER);
        let bytes = tokio::time::timeout(TIMEOUT, body.collect())
            .await
            .map_err(|_| BillingError::Unavailable)?
            .map_err(|_| BillingError::Unavailable)?
            .to_bytes();
        Ok((status, bytes))
    }
}

/// `application/x-www-form-urlencoded`, keys and values percent-encoded.
pub(crate) fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The JSON payload of a JWS, read without checking its signature: what it
/// says is only trusted when it came from the provider over TLS (§3).
pub(crate) fn jws_payload(jws: &str) -> Option<serde_json::Value> {
    let mut parts = jws.split('.');
    let (_, payload, _, None) = (parts.next()?, parts.next()?, parts.next()?, parts.next()) else {
        return None;
    };
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()
}

pub(crate) fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// An RFC 3339 time in UTC (`2026-11-07T10:00:00.123Z`, as Google writes
/// them) as Unix milliseconds.
pub(crate) fn rfc3339_ms(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 20 || *b.last()? != b'Z' || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<i64> {
        let s = text.get(range)?;
        s.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| s.parse().ok())?
    };
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    if second > 60 {
        return None;
    }
    let millis = match &text[19..text.len() - 1] {
        "" => 0,
        fraction => {
            let digits = fraction.strip_prefix('.')?;
            if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let ms: String = digits.chars().chain("000".chars()).take(3).collect();
            ms.parse().ok()?
        }
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000 + millis)
}

/// Digits after the decimal point in a currency's minor unit (ISO 4217).
fn minor_digits(currency: &str) -> u32 {
    const ZERO: &[&str] = &[
        "BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "PYG", "RWF", "UGX", "UYI", "VND",
        "VUV", "XAF", "XOF", "XPF",
    ];
    const THREE: &[&str] = &["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"];
    if ZERO.contains(&currency) {
        0
    } else if THREE.contains(&currency) {
        3
    } else {
        2
    }
}

/// An amount given in units of 10^-`scale` of the currency, in its minor
/// units (rounded down), or `None` if negative or too large.
pub(crate) fn minor_units(amount: i64, scale: u32, currency: &str) -> Option<i64> {
    let digits = minor_digits(currency);
    if amount < 0 || digits > scale {
        return None;
    }
    Some(amount / 10i64.checked_pow(scale - digits)?)
}

/// A three-letter currency code in upper case.
pub(crate) fn currency(text: &str) -> Option<String> {
    let upper = text.to_ascii_uppercase();
    (upper.len() == 3 && upper.bytes().all(|b| b.is_ascii_uppercase())).then_some(upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_are_percent_encoded() {
        assert_eq!(
            form(&[("a[0][b]", "x y&z"), ("url", "https://e.test/?q=1")]),
            "a%5B0%5D%5Bb%5D=x%20y%26z&url=https%3A%2F%2Fe.test%2F%3Fq%3D1"
        );
    }

    #[test]
    fn rfc3339_times_are_read_in_utc() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            rfc3339_ms("2026-11-07T10:00:00.123Z"),
            Some(1_794_045_600_123)
        );
        assert_eq!(rfc3339_ms("2000-02-29T23:59:59.5Z"), Some(951_868_799_500));
        assert_eq!(
            rfc3339_ms("2024-03-01T00:00:00.123456789Z"),
            Some(1_709_251_200_123)
        );
        for bad in [
            "",
            "2026-11-07T10:00:00",
            "2026-11-07T10:00:00+02:00",
            "2026-13-07T10:00:00Z",
            "2026-11-07 10:00:00Z",
            "2026-11-07T10:00:00.Z",
            "2026-1a-07T10:00:00Z",
        ] {
            assert_eq!(rfc3339_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn amounts_become_minor_units() {
        assert_eq!(minor_units(23_990, 3, "EUR"), Some(2399));
        assert_eq!(minor_units(3_000_000, 3, "JPY"), Some(3000));
        assert_eq!(minor_units(9_990, 3, "KWD"), Some(9990));
        assert_eq!(minor_units(24_990_000_000, 9, "USD"), Some(2499));
        assert_eq!(minor_units(-1, 3, "USD"), None);
        assert_eq!(currency("usd").as_deref(), Some("USD"));
        assert_eq!(currency("us"), None);
    }

    #[test]
    fn jws_payloads_are_read() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"a":1}"#);
        assert_eq!(jws_payload(&format!("e30.{payload}.c2ln")).unwrap()["a"], 1);
        assert!(jws_payload("e30.e30").is_none());
        assert!(jws_payload("e30.e30.c2ln.extra").is_none());
        assert!(jws_payload("e30.!!.c2ln").is_none());
    }
}
