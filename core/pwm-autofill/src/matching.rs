//! Which items a page may get, ranked, and whether to fill, warn or refuse
//! (D33 §3–§5).

use std::cmp::Reverse;

use pwm_vault::{LoginUrl, UrlMatch};
use regex::RegexBuilder;
use url::{Host, Url};

use crate::{AutofillError, Rules, registrable_domain};

/// Longest regex pattern an item may carry.
const MAX_PATTERN: usize = 1_000;
/// Compiled-regex size limit, so a pattern cannot use much memory.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// How an item URL matched, weakest first (so the derived order ranks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Match {
    Regex,
    Domain,
    Host,
    StartsWith,
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarnReason {
    /// The page is plain `http://`.
    InsecurePage,
    /// Only a `regex` URL matched.
    RegexOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefuseReason {
    /// The field is in a frame from another site that the item does not match.
    CrossSiteFrame,
    /// The signed rules mark this site "never fill".
    SiteBlocked,
}

/// What to do with a candidate. Every fill still needs a user action
/// (`AUTOFILL.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Fill,
    /// Fill only after the user sees a warning.
    Warn(WarnReason),
    Refuse(RefuseReason),
    /// Cards, identities and passkeys: always a confirmation step.
    Confirm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate<K> {
    pub key: K,
    pub matched: Match,
    pub decision: Decision,
}

fn is_web(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// The site a URL belongs to: its registrable domain, or the host itself
/// for IP addresses and hosts that have none (`localhost`, a public suffix).
pub(crate) fn site_of(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(domain) => {
            Some(registrable_domain(domain).unwrap_or_else(|| domain.to_owned()))
        }
        Host::Ipv4(ip) => Some(ip.to_string()),
        Host::Ipv6(ip) => Some(ip.to_string()),
    }
}

/// An item URL as stored ("example.com" or a full URL), parsed.
fn item_url(text: &str) -> Option<Url> {
    let text = text.trim();
    if text.contains("://") {
        Url::parse(text).ok()
    } else {
        Url::parse(&format!("https://{text}")).ok()
    }
}

fn without_fragment(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.into()
}

fn regex_matches(pattern: &str, page: &Url) -> bool {
    if pattern.chars().count() > MAX_PATTERN {
        return false;
    }
    // The regex crate runs in linear time: no catastrophic backtracking.
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .build()
        .is_ok_and(|re| re.is_match(&without_fragment(page)))
}

/// Whether one item URL in `mode` matches the page, and how.
pub fn match_url(page: &Url, item: &str, mode: &UrlMatch) -> Option<Match> {
    if !is_web(page) {
        return None;
    }
    if *mode == UrlMatch::Regex {
        return regex_matches(item, page).then_some(Match::Regex);
    }
    let item = item_url(item)?;
    let matched = match mode {
        UrlMatch::Domain => site_of(page).is_some() && site_of(page) == site_of(&item),
        UrlMatch::Host => {
            page.host_str() == item.host_str()
                && (item.port().is_none()
                    || page.port_or_known_default() == item.port_or_known_default())
        }
        UrlMatch::StartsWith => without_fragment(page).starts_with(&without_fragment(&item)),
        UrlMatch::Exact => without_fragment(page) == without_fragment(&item),
        // `Never`, and modes from a newer app version, never match.
        _ => false,
    };
    matched.then_some(match mode {
        UrlMatch::Domain => Match::Domain,
        UrlMatch::Host => Match::Host,
        UrlMatch::StartsWith => Match::StartsWith,
        _ => Match::Exact,
    })
}

/// The best way any of an item's URLs matches the page.
pub fn best_match(page: &Url, urls: &[LoginUrl]) -> Option<Match> {
    urls.iter()
        .filter_map(|u| match_url(page, &u.url, &u.r#match))
        .max()
}

fn parse(text: &str) -> Result<Url, AutofillError> {
    Url::parse(text).map_err(|_| AutofillError::InvalidUrl)
}

/// The candidates for a fill, best first: each item is
/// `(key, urls, last used)`. `frame_url` is the frame holding the fields,
/// if it is not the top page.
pub fn rank<K: Clone>(
    page_url: &str,
    frame_url: Option<&str>,
    items: &[(K, &[LoginUrl], i64)],
    rules: &Rules,
) -> Result<Vec<Candidate<K>>, AutofillError> {
    let top = parse(page_url)?;
    let frame = frame_url.map(parse).transpose()?;
    let target = frame.as_ref().unwrap_or(&top);
    let cross_site = frame.as_ref().is_some_and(|f| site_of(f) != site_of(&top));
    let blocked = rules.site_for(target).is_some_and(|s| s.never_fill);
    let mut out: Vec<(Candidate<K>, i64)> = Vec::new();
    for (key, urls, last_used) in items {
        let candidate = if let Some(matched) = best_match(target, urls) {
            let decision = if blocked {
                Decision::Refuse(RefuseReason::SiteBlocked)
            } else if target.scheme() == "http" {
                Decision::Warn(WarnReason::InsecurePage)
            } else if matched == Match::Regex {
                Decision::Warn(WarnReason::RegexOnly)
            } else {
                Decision::Fill
            };
            Candidate {
                key: key.clone(),
                matched,
                decision,
            }
        } else if let Some(matched) = cross_site.then(|| best_match(&top, urls)).flatten() {
            // It belongs to the top page, but the fields are another site's.
            Candidate {
                key: key.clone(),
                matched,
                decision: Decision::Refuse(RefuseReason::CrossSiteFrame),
            }
        } else {
            continue;
        };
        out.push((candidate, *last_used));
    }
    out.sort_by_key(|(c, last_used)| (Reverse(c.matched), Reverse(*last_used)));
    Ok(out.into_iter().map(|(c, _)| c).collect())
}
