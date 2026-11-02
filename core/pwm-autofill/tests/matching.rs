//! URL match modes, ranking and fill decisions (D33 §3–§5).

use pwm_autofill::{Decision, Match, RefuseReason, Rules, WarnReason, best_match, match_url, rank};
use pwm_vault::{LoginUrl, UrlMatch};

fn page(url: &str) -> url::Url {
    url::Url::parse(url).unwrap()
}

fn login_url(url: &str, mode: UrlMatch) -> LoginUrl {
    let mut login_url = LoginUrl::default();
    login_url.url = url.to_owned();
    login_url.r#match = mode;
    login_url
}

#[test]
fn each_mode_matches_what_it_says() {
    use UrlMatch::*;
    let cases: &[(&str, &str, UrlMatch, Option<Match>)] = &[
        // domain: the registrable domain, any subdomain, http or https.
        (
            "https://login.example.com/a",
            "example.com",
            Domain,
            Some(Match::Domain),
        ),
        (
            "https://example.com",
            "https://www.example.com/x",
            Domain,
            Some(Match::Domain),
        ),
        (
            "https://example.co.uk",
            "shop.example.co.uk",
            Domain,
            Some(Match::Domain),
        ),
        ("https://evil-example.com", "example.com", Domain, None),
        ("https://example.com.evil.net", "example.com", Domain, None),
        ("https://alice.github.io", "bob.github.io", Domain, None),
        // IDN: the page in Unicode, the item in punycode (and back).
        (
            "https://www.παράδειγμα.ελ/",
            "xn--hxajbheg2az3al.xn--qxam",
            Domain,
            Some(Match::Domain),
        ),
        (
            "https://xn--hxajbheg2az3al.xn--qxam/",
            "παράδειγμα.ελ",
            Domain,
            Some(Match::Domain),
        ),
        // IP addresses and single-label hosts match only themselves.
        (
            "http://192.168.1.10:8080/",
            "192.168.1.10",
            Domain,
            Some(Match::Domain),
        ),
        ("http://192.168.1.11/", "192.168.1.10", Domain, None),
        (
            "http://localhost:3000/",
            "localhost",
            Domain,
            Some(Match::Domain),
        ),
        ("http://intranet/", "localhost", Domain, None),
        // host: exact host, and port when given.
        (
            "https://login.example.com/",
            "login.example.com",
            Host,
            Some(Match::Host),
        ),
        ("https://www.example.com/", "login.example.com", Host, None),
        (
            "https://example.com:8443/",
            "https://example.com:8443",
            Host,
            Some(Match::Host),
        ),
        (
            "https://example.com/",
            "https://example.com:8443",
            Host,
            None,
        ),
        (
            "https://example.com:443/",
            "https://example.com",
            Host,
            Some(Match::Host),
        ),
        // starts_with and exact: normalized URLs, fragment ignored.
        (
            "https://example.com/app/login?x=1",
            "https://example.com/app",
            StartsWith,
            Some(Match::StartsWith),
        ),
        (
            "https://example.com/other",
            "https://example.com/app",
            StartsWith,
            None,
        ),
        (
            "https://EXAMPLE.com/login#top",
            "https://example.com/login",
            Exact,
            Some(Match::Exact),
        ),
        (
            "https://example.com/login?x=1",
            "https://example.com/login",
            Exact,
            None,
        ),
        // regex: the whole URL.
        (
            "https://a1.example.com/",
            r"^https://a\d\.example\.com/$",
            Regex,
            Some(Match::Regex),
        ),
        (
            "https://ab.example.com/",
            r"^https://a\d\.example\.com/$",
            Regex,
            None,
        ),
        ("https://example.com/", "(", Regex, None),
        // never.
        ("https://example.com/", "example.com", Never, None),
        // Only http and https pages.
        ("ftp://example.com/", "example.com", Domain, None),
        (
            "file:///C:/login.html",
            "file:///C:/login.html",
            Exact,
            None,
        ),
        ("javascript:alert(1)", "example.com", Domain, None),
    ];
    for (page_url, item_url, mode, expected) in cases {
        assert_eq!(
            match_url(&page(page_url), item_url, mode),
            *expected,
            "{page_url} vs {item_url} ({mode:?})"
        );
    }
}

#[test]
fn a_catastrophic_regex_stays_fast_and_long_patterns_are_refused() {
    let evil = "^(a+)+$";
    let url = format!("https://example.com/{}b", "a".repeat(5_000));
    let started = std::time::Instant::now();
    assert_eq!(match_url(&page(&url), evil, &UrlMatch::Regex), None);
    assert!(
        started.elapsed().as_millis() < 500,
        "regex took {:?}",
        started.elapsed()
    );
    let long = format!("^https://example\\.com/{}", "a".repeat(1_000));
    assert_eq!(
        match_url(&page("https://example.com/"), &long, &UrlMatch::Regex),
        None
    );
}

#[test]
fn the_best_url_of_an_item_wins() {
    let urls = [
        login_url("example.com", UrlMatch::Domain),
        login_url("https://example.com/login", UrlMatch::Exact),
    ];
    assert_eq!(
        best_match(&page("https://example.com/login"), &urls),
        Some(Match::Exact)
    );
    assert_eq!(
        best_match(&page("https://example.com/other"), &urls),
        Some(Match::Domain)
    );
    assert_eq!(best_match(&page("https://other.com/"), &urls), None);
}

fn ranked(
    page_url: &str,
    frame: Option<&str>,
    items: &[(&str, Vec<LoginUrl>, i64)],
    rules: &Rules,
) -> Vec<(String, Match, Decision)> {
    let items: Vec<(String, &[LoginUrl], i64)> = items
        .iter()
        .map(|(k, u, t)| ((*k).to_owned(), u.as_slice(), *t))
        .collect();
    rank(page_url, frame, &items, rules)
        .unwrap()
        .into_iter()
        .map(|c| (c.key, c.matched, c.decision))
        .collect()
}

#[test]
fn candidates_are_ranked_and_decided() {
    let rules = Rules::empty();
    let items = [
        (
            "domain-old",
            vec![login_url("example.com", UrlMatch::Domain)],
            10,
        ),
        (
            "domain-new",
            vec![login_url("example.com", UrlMatch::Domain)],
            20,
        ),
        (
            "exact",
            vec![login_url("https://www.example.com/login", UrlMatch::Exact)],
            1,
        ),
        (
            "regex",
            vec![login_url(r"^https://www\.example\.com/", UrlMatch::Regex)],
            99,
        ),
        ("never", vec![login_url("example.com", UrlMatch::Never)], 99),
        ("other", vec![login_url("other.com", UrlMatch::Domain)], 99),
    ];
    let got = ranked("https://www.example.com/login", None, &items, &rules);
    let keys: Vec<&str> = got.iter().map(|(k, _, _)| k.as_str()).collect();
    assert_eq!(keys, ["exact", "domain-new", "domain-old", "regex"]);
    assert_eq!(got[0].2, Decision::Fill);
    assert_eq!(got[3].2, Decision::Warn(WarnReason::RegexOnly));

    // A plain-http page is a warning, never a silent fill.
    let got = ranked("http://www.example.com/login", None, &items, &rules);
    assert!(got.iter().all(|(_, _, d)| matches!(d, Decision::Warn(_))));
    assert_eq!(got[0].2, Decision::Warn(WarnReason::InsecurePage));
}

#[test]
fn frames_from_another_site_are_refused_unless_the_item_matches_the_frame() {
    let rules = Rules::empty();
    let items = [
        ("shop", vec![login_url("shop.com", UrlMatch::Domain)], 1),
        (
            "payments",
            vec![login_url("pay.example", UrlMatch::Domain)],
            1,
        ),
    ];
    // The login form sits in a pay.example frame inside shop.com.
    let got = ranked(
        "https://shop.com/checkout",
        Some("https://secure.pay.example/frame"),
        &items,
        &rules,
    );
    let shop = got.iter().find(|(k, _, _)| k == "shop").unwrap();
    assert_eq!(shop.2, Decision::Refuse(RefuseReason::CrossSiteFrame));
    let payments = got.iter().find(|(k, _, _)| k == "payments").unwrap();
    assert_eq!(payments.2, Decision::Fill);
    // A same-site frame is fine.
    let got = ranked(
        "https://shop.com/",
        Some("https://login.shop.com/f"),
        &items,
        &rules,
    );
    assert_eq!(got[0], ("shop".to_owned(), Match::Domain, Decision::Fill));
}

#[test]
fn bad_page_urls_are_errors() {
    let items: Vec<(String, &[LoginUrl], i64)> = Vec::new();
    assert!(rank("not a url", None, &items, &Rules::empty()).is_err());
    assert!(rank("https://example.com", Some("::"), &items, &Rules::empty()).is_err());
}
