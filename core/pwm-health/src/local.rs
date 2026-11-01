use std::collections::HashMap;

use pwm_crypto::ReuseHasher;
use pwm_generator::{MIN_SIGNUP_SCORE, strength};
use pwm_vault::{Item, ItemId, Login};

use crate::{HealthError, candidates};

/// Words shorter than this are too common to be useful as context.
const MIN_CONTEXT_WORD: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeakPassword {
    pub item_id: ItemId,
    /// zxcvbn score, below [`MIN_SIGNUP_SCORE`].
    pub score: u8,
}

/// Results of the checks that never leave the device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalReport {
    /// In input order.
    pub weak: Vec<WeakPassword>,
    /// Groups of items sharing one password; each group sorted by id, groups
    /// sorted by their first id.
    pub reused: Vec<Vec<ItemId>>,
}

/// Finds weak and reused passwords.
pub fn check_local(items: &[(ItemId, &Item)]) -> Result<LocalReport, HealthError> {
    let hasher = ReuseHasher::new()?;
    let mut report = LocalReport::default();
    let mut by_hash: HashMap<[u8; 32], Vec<ItemId>> = HashMap::new();
    for (item_id, item, login) in candidates(items) {
        let words = context_words(item, login);
        let context: Vec<&str> = words.iter().map(String::as_str).collect();
        let score = strength(&login.password, &context).score;
        if score < MIN_SIGNUP_SCORE {
            report.weak.push(WeakPassword { item_id, score });
        }
        by_hash
            .entry(hasher.hash(login.password.as_bytes()))
            .or_default()
            .push(item_id);
    }
    report.reused = by_hash
        .into_values()
        .filter(|ids| ids.len() > 1)
        .map(|mut ids| {
            ids.sort();
            ids
        })
        .collect();
    report.reused.sort();
    Ok(report)
}

/// Words a user is likely to build a password from: the title, the username
/// (including an email's parts) and each URL's host.
fn context_words(item: &Item, login: &Login) -> Vec<String> {
    let mut sources = vec![item.title.as_str(), login.username.as_str()];
    sources.extend(login.urls.iter().map(|u| host(&u.url)));
    sources
        .into_iter()
        .flat_map(|s| s.split(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.chars().count() >= MIN_CONTEXT_WORD)
        .map(str::to_lowercase)
        .collect()
}

/// The host of a URL or bare domain, without scheme, user info, port or path.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    host.split(':').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::host;

    #[test]
    fn hosts() {
        assert_eq!(host("https://www.example.com/login?x=1"), "www.example.com");
        assert_eq!(host("example.org"), "example.org");
        assert_eq!(
            host("https://user:pw@mail.example.net:8443/"),
            "mail.example.net"
        );
        assert_eq!(host("http://host#frag"), "host");
        assert_eq!(host(""), "");
    }
}
