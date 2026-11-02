//! The Public Suffix List (D33 §2): the vendored list, ICANN and private
//! sections both, compiled in and parsed once.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::LazyLock;

const LIST: &str = include_str!("../data/public_suffix_list.dat");

/// The `VERSION` line of the vendored list (see `data/README.md`).
pub const PSL_VERSION: &str = "2026-10-31_14-17-28_UTC";

struct Rules {
    exact: HashSet<String>,
    /// `*.ck` is stored as `ck`.
    wildcard: HashSet<String>,
    /// `!www.ck` is stored as `www.ck`.
    exception: HashSet<String>,
}

static RULES: LazyLock<Rules> = LazyLock::new(|| {
    let mut rules = Rules {
        exact: HashSet::new(),
        wildcard: HashSet::new(),
        exception: HashSet::new(),
    };
    for line in LIST.lines() {
        let Some(rule) = line.split_whitespace().next() else {
            continue;
        };
        if rule.starts_with("//") {
            continue;
        }
        let (set, name) = if let Some(rest) = rule.strip_prefix('!') {
            (&mut rules.exception, rest)
        } else if let Some(rest) = rule.strip_prefix("*.") {
            (&mut rules.wildcard, rest)
        } else {
            (&mut rules.exact, rule)
        };
        // The list has Unicode rules; hosts arrive in ASCII (punycode).
        if let Ok(ascii) = idna::domain_to_ascii(name) {
            set.insert(ascii);
        }
    }
    rules
});

/// How many trailing labels of `labels` form the public suffix.
fn suffix_len(labels: &[&str]) -> usize {
    let rules = &*RULES;
    let n = labels.len();
    let joined = |k: usize| labels[n - k..].join(".");
    // An exception rule wins over everything; its suffix is one label shorter.
    for k in (1..=n).rev() {
        if rules.exception.contains(&joined(k)) {
            return k - 1;
        }
    }
    for k in (1..=n).rev() {
        if rules.exact.contains(&joined(k)) || (k >= 2 && rules.wildcard.contains(&joined(k - 1))) {
            return k;
        }
    }
    // The default rule `*`: the last label is the suffix.
    1
}

fn label_ok(label: &str) -> bool {
    !label.is_empty()
        && label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The registrable domain of `host` (the public suffix plus one label), in
/// ASCII: `www.example.co.uk` → `example.co.uk`, `alice.github.io` →
/// `alice.github.io`. `None` for a public suffix itself, an IP address or
/// anything that is not a domain name.
pub fn registrable_domain(host: &str) -> Option<String> {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty()
        || host.starts_with('.')
        || host.starts_with('[')
        || host.parse::<IpAddr>().is_ok()
    {
        return None;
    }
    let ascii = idna::domain_to_ascii(host).ok()?;
    let labels: Vec<&str> = ascii.split('.').collect();
    if !labels.iter().all(|l| label_ok(l)) {
        return None;
    }
    let suffix = suffix_len(&labels);
    if labels.len() <= suffix {
        return None;
    }
    Some(labels[labels.len() - suffix - 1..].join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constant_names_the_vendored_version() {
        assert!(LIST.contains(&format!("// VERSION: {PSL_VERSION}\n")));
    }

    #[test]
    fn both_sections_are_loaded() {
        assert!(RULES.exact.contains("co.uk"));
        assert!(RULES.exact.contains("github.io"));
        assert!(RULES.wildcard.contains("ck"));
        assert!(RULES.exception.contains("www.ck"));
        assert!(RULES.exact.len() > 8_000);
    }
}
