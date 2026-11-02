//! Multi-step login state (D33 §8): per tab, no values, forgotten after
//! 5 minutes, a site change or the tab closing.

use pwm_autofill::{MultiStep, NextStep, StepDone};

const T0: i64 = 1_767_225_600_000;
const MINUTE: i64 = 60_000;

#[test]
fn the_next_page_of_a_login_gets_the_same_item() {
    let mut steps = MultiStep::new();
    steps
        .record(
            "tab-1",
            "https://accounts.example.com/signin",
            "item-a",
            StepDone::Username,
            T0,
        )
        .unwrap();
    // The password page, on another host of the same site.
    let next = steps
        .next("tab-1", "https://login.example.com/pwd", T0 + MINUTE)
        .unwrap();
    assert_eq!(next, ("item-a".to_owned(), NextStep::Password));
    steps
        .record(
            "tab-1",
            "https://login.example.com/pwd",
            "item-a",
            StepDone::Password,
            T0 + MINUTE,
        )
        .unwrap();
    let next = steps
        .next("tab-1", "https://login.example.com/2fa", T0 + 2 * MINUTE)
        .unwrap();
    assert_eq!(next, ("item-a".to_owned(), NextStep::Totp));
    // Other tabs know nothing.
    assert_eq!(
        steps.next("tab-2", "https://login.example.com/pwd", T0 + MINUTE),
        None
    );
}

#[test]
fn state_is_forgotten() {
    let mut steps = MultiStep::new();
    steps
        .record(
            "tab",
            "https://example.com/a",
            "item",
            StepDone::Username,
            T0,
        )
        .unwrap();
    // After 5 minutes.
    assert!(
        steps
            .next("tab", "https://example.com/b", T0 + 5 * MINUTE - 1)
            .is_some()
    );
    assert_eq!(
        steps.next("tab", "https://example.com/b", T0 + 5 * MINUTE),
        None
    );

    // When the tab goes to another site: and it stays forgotten.
    steps
        .record(
            "tab",
            "https://example.com/a",
            "item",
            StepDone::Username,
            T0,
        )
        .unwrap();
    assert_eq!(
        steps.next("tab", "https://evil.example.net/pwd", T0 + 1),
        None
    );
    assert_eq!(steps.next("tab", "https://example.com/pwd", T0 + 2), None);

    // When the tab closes.
    steps
        .record(
            "tab",
            "https://example.com/a",
            "item",
            StepDone::Username,
            T0,
        )
        .unwrap();
    steps.forget("tab");
    assert_eq!(steps.next("tab", "https://example.com/pwd", T0 + 1), None);

    // Bad URLs are refused, and only http(s) pages count.
    assert!(
        steps
            .record("tab", "nope", "item", StepDone::Username, T0)
            .is_err()
    );
    assert!(
        steps
            .record("tab", "file:///x", "item", StepDone::Username, T0)
            .is_err()
    );
}

#[test]
fn many_tabs_do_not_grow_without_bound() {
    let mut steps = MultiStep::new();
    for n in 0..5_000 {
        steps
            .record(
                &format!("tab-{n}"),
                "https://example.com/",
                "item",
                StepDone::Username,
                T0 + n,
            )
            .unwrap();
    }
    assert!(steps.len() <= 1_000, "{}", steps.len());
    // The newest are kept.
    assert!(
        steps
            .next("tab-4999", "https://example.com/p", T0 + 5_000)
            .is_some()
    );
}
