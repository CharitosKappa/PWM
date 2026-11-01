use std::fmt;

use pwm_crypto::{Secret, hibp_sha1};
use pwm_vault::{Item, ItemId};
use zeroize::Zeroizing;

use crate::{HealthError, candidates};

/// Base URL for range requests: append the 5-character prefix.
pub const HIBP_RANGE_URL: &str = "https://api.pwnedpasswords.com/range/";

/// Header every range request must send, so the response size does not hint
/// at the prefix. Requests carry no cookies or account identifiers.
pub const ADD_PADDING_HEADER: (&str, &str) = ("Add-Padding", "true");

const PREFIX_LEN: usize = 5;
const SUFFIX_LEN: usize = 35;

/// Fetches `HIBP_RANGE_URL + prefix` with [`ADD_PADDING_HEADER`] and returns
/// the response body. Implemented by each platform.
pub trait RangeFetcher {
    fn fetch_range(&mut self, prefix: &str) -> Result<String, HealthError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreachedPassword {
    pub item_id: ItemId,
    /// How many times HIBP has seen this password in breaches.
    pub count: u64,
}

/// A breach check without I/O: ask for [`prefixes`](Self::prefixes), feed
/// each range response to [`add_response`](Self::add_response), then
/// [`finish`](Self::finish). Hashes are zeroized when the check is dropped.
pub struct BreachCheck {
    entries: Vec<Entry>,
}

struct Entry {
    item_id: ItemId,
    hash: Secret<[u8; 20]>,
    count: u64,
}

impl BreachCheck {
    pub fn new(items: &[(ItemId, &Item)]) -> Result<Self, HealthError> {
        let entries = candidates(items)
            .map(|(item_id, _, login)| Entry {
                item_id,
                hash: hibp_sha1(login.password.as_bytes()),
                count: 0,
            })
            .collect();
        Ok(Self { entries })
    }

    /// The distinct 5-character uppercase hex prefixes to request.
    pub fn prefixes(&self) -> Vec<String> {
        let mut prefixes: Vec<String> = Vec::new();
        for entry in &self.entries {
            let hex = hex_upper(entry.hash.expose_secret());
            let prefix = std::str::from_utf8(&hex[..PREFIX_LEN]).expect("hex is ASCII");
            if !prefixes.iter().any(|p| p == prefix) {
                prefixes.push(prefix.to_owned());
            }
        }
        prefixes
    }

    /// Matches a range response (`SUFFIX:COUNT` lines) for `prefix`. Padding
    /// rows (count 0) never count as a breach.
    pub fn add_response(&mut self, prefix: &str, body: &str) -> Result<(), HealthError> {
        let rows = parse_range(body)?;
        let mut requested = false;
        for entry in &mut self.entries {
            let hex = hex_upper(entry.hash.expose_secret());
            if !hex[..PREFIX_LEN].eq_ignore_ascii_case(prefix.as_bytes()) {
                continue;
            }
            requested = true;
            if let Some((_, count)) = rows
                .iter()
                .find(|(suffix, _)| suffix.eq_ignore_ascii_case(&hex[PREFIX_LEN..]))
            {
                entry.count = *count;
            }
        }
        if requested {
            Ok(())
        } else {
            Err(HealthError::UnexpectedPrefix)
        }
    }

    /// Breached items, in input order. Prefixes never answered count as not
    /// breached.
    pub fn finish(self) -> Vec<BreachedPassword> {
        self.entries
            .iter()
            .filter(|e| e.count > 0)
            .map(|e| BreachedPassword {
                item_id: e.item_id,
                count: e.count,
            })
            .collect()
    }
}

impl fmt::Debug for BreachCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BreachCheck")
            .field("items", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// Runs a full breach check, one request per distinct prefix.
pub fn check_breaches(
    items: &[(ItemId, &Item)],
    fetcher: &mut impl RangeFetcher,
) -> Result<Vec<BreachedPassword>, HealthError> {
    let mut check = BreachCheck::new(items)?;
    for prefix in check.prefixes() {
        let body = fetcher.fetch_range(&prefix)?;
        check.add_response(&prefix, &body)?;
    }
    Ok(check.finish())
}

fn parse_range(body: &str) -> Result<Vec<(&[u8], u64)>, HealthError> {
    body.lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (suffix, count) = line.split_once(':').ok_or(HealthError::BadResponse)?;
            if suffix.len() != SUFFIX_LEN || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(HealthError::BadResponse);
            }
            let count = count.trim().parse().map_err(|_| HealthError::BadResponse)?;
            Ok((suffix.as_bytes(), count))
        })
        .collect()
}

fn hex_upper(bytes: &[u8; 20]) -> Zeroizing<[u8; 40]> {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Zeroizing::new([0u8; 40]);
    for (i, b) in bytes.iter().enumerate() {
        out[2 * i] = DIGITS[usize::from(b >> 4)];
        out[2 * i + 1] = DIGITS[usize::from(b & 0x0F)];
    }
    out
}
