//! The screens' protocol samples (D66 §3), from `ui/src/protocol-samples.json`:
//! every request parses, the desktop's kinds as themselves and the rest as
//! `Unsupported`, and the replies the backend builds read and write back
//! the same JSON.

use pwm_desktop::protocol::{AskView, ItemSummary, Request, Settings, Status};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn samples() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../ui/src/protocol-samples.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("the samples")).expect("JSON")
}

#[test]
fn every_request_parses_and_only_the_desktops_are_supported() {
    let samples = samples();
    let desktop: Vec<&str> = samples["desktopKinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    let requests = samples["requests"].as_array().unwrap();
    assert!(requests.len() > desktop.len());
    for request in requests {
        let kind = request["kind"].as_str().unwrap();
        let parsed: Request =
            serde_json::from_value(request.clone()).unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert_eq!(
            !matches!(parsed, Request::Unsupported),
            desktop.contains(&kind),
            "{kind}"
        );
    }
}

/// Each sample reads as `T` and writes back as the same JSON.
fn round_trip<T: DeserializeOwned + Serialize>(kind: &str) {
    let samples = samples();
    let replies = samples["replies"][kind]
        .as_array()
        .unwrap_or_else(|| panic!("{kind}"));
    assert!(!replies.is_empty());
    for reply in replies {
        let parsed: T =
            serde_json::from_value(reply.clone()).unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert_eq!(&serde_json::to_value(&parsed).unwrap(), reply, "{kind}");
    }
}

#[test]
fn the_replies_match_the_screens_types() {
    round_trip::<Status>("status");
    round_trip::<Vec<ItemSummary>>("items");
    round_trip::<Settings>("settings");
    round_trip::<AskView>("askApproval");
}

#[test]
fn a_request_missing_a_field_or_with_a_bad_value_is_refused() {
    for bad in [
        r#"{"kind":"unlock"}"#,
        r#"{"kind":"copy","vaultId":"v","itemId":"i","field":"notes"}"#,
        r#"{"kind":"generate","mode":"pin"}"#,
        r#"{"nokind":true}"#,
    ] {
        assert!(serde_json::from_str::<Request>(bad).is_err(), "{bad}");
    }
}
