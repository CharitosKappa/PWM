//! The Public Suffix List matcher (D33) against the list's official test
//! cases, and the vendored file against its recorded version.

use pwm_autofill::{PSL_VERSION, registrable_domain};

const TESTS: &str = include_str!("../data/test_psl.txt");
const README: &str = include_str!("../data/README.md");

/// `checkPublicSuffix('input', 'expected')` or `..., null)`.
fn cases() -> Vec<(String, Option<String>)> {
    TESTS
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("checkPublicSuffix("))
        .map(|l| {
            let inner = &l["checkPublicSuffix(".len()..l.len() - 2];
            let (input, expected) = inner.split_once(", ").unwrap();
            let text = |s: &str| (s != "null").then(|| s.trim_matches('\'').to_owned());
            (input.to_owned(), text(expected))
        })
        .collect()
}

#[test]
fn the_official_test_cases_pass() {
    let cases = cases();
    assert!(cases.len() > 70, "{} cases", cases.len());
    for (input, expected) in cases {
        if input == "null" {
            continue;
        }
        let input = input.trim_matches('\'');
        // Results are in ASCII (punycode); compare like with like.
        let expected = expected.map(|e| idna::domain_to_ascii(&e).unwrap());
        assert_eq!(registrable_domain(input), expected, "{input}");
    }
}

#[test]
fn private_domains_are_separate_sites() {
    assert_eq!(
        registrable_domain("alice.github.io").as_deref(),
        Some("alice.github.io")
    );
    assert_eq!(
        registrable_domain("bob.github.io").as_deref(),
        Some("bob.github.io")
    );
    assert_eq!(registrable_domain("github.io"), None);
    assert_eq!(
        registrable_domain("www.example.co.uk").as_deref(),
        Some("example.co.uk")
    );
}

#[test]
fn hosts_that_are_not_domains_have_none() {
    for host in [
        "",
        ".",
        "..example.com",
        "example..com",
        "127.0.0.1",
        "[::1]",
        "a b.com",
    ] {
        assert_eq!(registrable_domain(host), None, "{host:?}");
    }
}

#[test]
fn the_vendored_list_matches_its_recorded_version() {
    assert!(PSL_VERSION.starts_with("20"), "{PSL_VERSION}");
    assert!(
        README.contains(PSL_VERSION),
        "data/README.md must record {PSL_VERSION}"
    );
}
