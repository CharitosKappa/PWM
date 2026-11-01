//! Weak, reused and breached checks (decision D22).

use pwm_health::{
    ADD_PADDING_HEADER, BreachCheck, BreachedPassword, HIBP_RANGE_URL, HealthError, RangeFetcher,
    check_breaches, check_local,
};
use pwm_vault::{Item, ItemId, ItemKind, Login, LoginUrl};

/// A password zxcvbn rates 4 on its own.
const STRONG: &str = "vT7#qLm2!xR9pWz4@kD";

fn login(n: u8, title: &str, username: &str, password: &str) -> (ItemId, Item) {
    let mut l = Login::default();
    l.username = username.into();
    l.password = password.into();
    (
        ItemId::from_bytes([n; 16]),
        Item::new(ItemKind::Login(l), title, 0),
    )
}

fn refs(items: &[(ItemId, Item)]) -> Vec<(ItemId, &Item)> {
    items.iter().map(|(id, item)| (*id, item)).collect()
}

fn id(n: u8) -> ItemId {
    ItemId::from_bytes([n; 16])
}

// --- Weak ---

#[test]
fn common_passwords_are_weak() {
    let items = [
        login(1, "Mail", "a", "password1"),
        login(2, "Bank", "b", STRONG),
        login(3, "Shop", "c", "qwerty123"),
    ];
    let report = check_local(&refs(&items)).unwrap();
    let weak: Vec<ItemId> = report.weak.iter().map(|w| w.item_id).collect();
    assert_eq!(weak, [id(1), id(3)]);
    assert!(report.weak.iter().all(|w| w.score < 3));
}

#[test]
fn passwords_built_from_the_item_are_weak() {
    let password = "ZorblaxianQuintessa";
    let built = [login(1, "Zorblaxian", "quintessa@example.com", password)];
    let unrelated = [login(1, "Mail", "someone@example.com", password)];
    assert_eq!(check_local(&refs(&built)).unwrap().weak.len(), 1);
    assert!(check_local(&refs(&unrelated)).unwrap().weak.is_empty());
}

#[test]
fn url_hosts_count_as_context() {
    let password = "PlumbrinkleVastopolous";
    let (item_id, mut item) = login(1, "Account", "x", password);
    if let ItemKind::Login(l) = &mut item.kind {
        let mut url = LoginUrl::default();
        url.url = "https://www.plumbrinkle.example/login?next=/".into();
        l.urls.push(url);
        let mut url = LoginUrl::default();
        url.url = "vastopolous.example".into();
        l.urls.push(url);
    }
    let with_urls = [(item_id, item)];
    let without = [login(1, "Account", "x", password)];
    assert_eq!(check_local(&refs(&with_urls)).unwrap().weak.len(), 1);
    assert!(check_local(&refs(&without)).unwrap().weak.is_empty());
}

#[test]
fn trashed_empty_and_non_login_items_are_skipped() {
    let (trashed_id, mut trashed) = login(1, "Old", "a", "password");
    trashed.trash(1).unwrap();
    let items = [
        (trashed_id, trashed),
        login(2, "No password", "b", ""),
        (id(3), Item::new(ItemKind::Note, "password", 0)),
    ];
    let report = check_local(&refs(&items)).unwrap();
    assert!(report.weak.is_empty());
    assert!(report.reused.is_empty());
}

// --- Reused ---

#[test]
fn reused_passwords_are_grouped() {
    let items = [
        login(4, "D", "d", STRONG),
        login(1, "A", "a", "unique-Password-#1x"),
        login(3, "C", "c", STRONG),
        login(2, "B", "b", STRONG),
        login(5, "E", "e", "Other-Shared-#22yy"),
        login(6, "F", "f", "Other-Shared-#22yy"),
    ];
    let report = check_local(&refs(&items)).unwrap();
    assert_eq!(
        report.reused,
        [vec![id(2), id(3), id(4)], vec![id(5), id(6)]]
    );
}

#[test]
fn reuse_is_case_sensitive_and_ignores_trash() {
    let (trashed_id, mut trashed) = login(3, "C", "c", STRONG);
    trashed.trash(1).unwrap();
    let items = [
        login(1, "A", "a", STRONG),
        login(2, "B", "b", &STRONG.to_lowercase()),
        (trashed_id, trashed),
    ];
    assert!(check_local(&refs(&items)).unwrap().reused.is_empty());
}

// --- Breached ---

/// Serves canned range responses and records every prefix requested.
struct FakeHibp {
    requested: Vec<String>,
    /// Full uppercase SHA-1 hex -> count; 0 adds a padding row.
    known: Vec<(&'static str, u64)>,
}

impl RangeFetcher for FakeHibp {
    fn fetch_range(&mut self, prefix: &str) -> Result<String, HealthError> {
        self.requested.push(prefix.to_owned());
        let mut body = String::from("0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n");
        for (hash, count) in &self.known {
            if let Some(suffix) = hash.strip_prefix(prefix) {
                body.push_str(&format!("{suffix}:{count}\r\n"));
            }
        }
        // Padding rows HIBP adds with Add-Padding: true.
        body.push_str("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF:0");
        Ok(body)
    }
}

const SHA1_PASSWORD: &str = "5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8";
const SHA1_HUNTER2: &str = "F3BBBD66A63D4BF1747940578EC3D0103530E21D";

#[test]
fn breached_passwords_are_found_with_prefixes_only() {
    let items = [
        login(1, "A", "a", "password"),
        login(2, "B", "b", STRONG),
        login(3, "C", "c", "hunter2"),
    ];
    let mut hibp = FakeHibp {
        requested: Vec::new(),
        known: vec![(SHA1_PASSWORD, 9_545_824), (SHA1_HUNTER2, 0)],
    };
    let breached = check_breaches(&refs(&items), &mut hibp).unwrap();
    assert_eq!(
        breached,
        [BreachedPassword {
            item_id: id(1),
            count: 9_545_824
        }]
    );
    assert_eq!(hibp.requested.len(), 3);
    assert!(hibp.requested.contains(&"5BAA6".to_owned()));
    for prefix in &hibp.requested {
        assert_eq!(prefix.len(), 5);
        assert!(
            prefix
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
        );
    }
}

#[test]
fn each_prefix_is_requested_once() {
    let items = [
        login(1, "A", "a", "password"),
        login(2, "B", "b", "password"),
        (id(3), {
            let (_, mut item) = login(3, "C", "c", "password");
            item.trash(1).unwrap();
            item
        }),
    ];
    let mut hibp = FakeHibp {
        requested: Vec::new(),
        known: vec![(SHA1_PASSWORD, 3)],
    };
    let breached = check_breaches(&refs(&items), &mut hibp).unwrap();
    assert_eq!(hibp.requested, ["5BAA6"]);
    let ids: Vec<ItemId> = breached.iter().map(|b| b.item_id).collect();
    assert_eq!(ids, [id(1), id(2)]);
}

#[test]
fn sans_io_check_accepts_lowercase_and_reports_unanswered_as_unknown() {
    let items = [
        login(1, "A", "a", "password"),
        login(2, "B", "b", "hunter2"),
    ];
    let mut check = BreachCheck::new(&refs(&items)).unwrap();
    let mut prefixes = check.prefixes();
    prefixes.sort();
    assert_eq!(prefixes, ["5BAA6", "F3BBB"]);
    check
        .add_response("5BAA6", "1e4c9b93f3f0682250b6cf8331b7ee68fd8:42\n")
        .unwrap();
    // F3BBB never answered: not reported as breached.
    assert_eq!(
        check.finish(),
        [BreachedPassword {
            item_id: id(1),
            count: 42
        }]
    );
}

#[test]
fn bad_responses_are_rejected() {
    let items = [login(1, "A", "a", "password")];
    let bad_bodies = [
        "not a range response",
        "1E4C9B93F3F0682250B6CF8331B7EE68FD8",
        "1E4C9B93F3F0682250B6CF8331B7EE68FD8:many",
        "1E4C9B93:5",
        "ZZZZZB93F3F0682250B6CF8331B7EE68FD8:5",
    ];
    for body in bad_bodies {
        let mut check = BreachCheck::new(&refs(&items)).unwrap();
        assert!(
            matches!(
                check.add_response("5BAA6", body),
                Err(HealthError::BadResponse)
            ),
            "{body}"
        );
    }
    let mut check = BreachCheck::new(&refs(&items)).unwrap();
    assert!(matches!(
        check.add_response("00000", ""),
        Err(HealthError::UnexpectedPrefix)
    ));
    // An empty body is a valid "no matches" answer.
    check.add_response("5BAA6", "").unwrap();
    assert!(check.finish().is_empty());
}

#[test]
fn fetch_errors_stop_the_check() {
    struct Offline;
    impl RangeFetcher for Offline {
        fn fetch_range(&mut self, _prefix: &str) -> Result<String, HealthError> {
            Err(HealthError::Fetch("offline".into()))
        }
    }
    let items = [login(1, "A", "a", "password")];
    assert!(matches!(
        check_breaches(&refs(&items), &mut Offline),
        Err(HealthError::Fetch(_))
    ));
}

#[test]
fn request_details_for_platforms() {
    assert_eq!(HIBP_RANGE_URL, "https://api.pwnedpasswords.com/range/");
    assert_eq!(ADD_PADDING_HEADER, ("Add-Padding", "true"));
}

#[test]
fn debug_output_hides_hashes() {
    let items = [login(1, "A", "a", "password")];
    let check = BreachCheck::new(&refs(&items)).unwrap();
    let debug = format!("{check:?}");
    assert!(!debug.contains("1E4C9B93"), "{debug}");
    assert!(!debug.to_lowercase().contains("1e4c9b93"), "{debug}");
}
