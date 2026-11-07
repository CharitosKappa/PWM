//! The handler with a fake Windows Hello key and a store in memory: what
//! the extension's `TouchIdUnlock` sees (D48 §5, D69).

use std::cell::RefCell;
use std::collections::HashMap;

use pwm_hello::FakeHello;
use serde_json::{Value, json};

use super::*;

const CHROME: &str = "chrome-extension://pppppppppppppppppppppppppppppppp/";
const FIREFOX: &str = "pwm@pwm.invalid";
const ACCOUNT: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const DEVICE: &str = "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1";

#[derive(Default)]
struct Memory(RefCell<HashMap<String, Vec<u8>>>);

impl Store for Memory {
    fn get(&self, id: &str) -> Option<Vec<u8>> {
        self.0.borrow().get(id).cloned()
    }
    fn set(&self, id: &str, record: &[u8]) -> Result<(), StoreError> {
        self.0.borrow_mut().insert(id.to_owned(), record.to_vec());
        Ok(())
    }
    fn delete(&self, id: &str) {
        self.0.borrow_mut().remove(id);
    }
    fn ids(&self) -> Vec<String> {
        self.0.borrow().keys().cloned().collect()
    }
}

fn key() -> String {
    STANDARD.encode([0x5A; 32])
}

fn request(kind: &str) -> Vec<u8> {
    let mut message = json!({"v": 1, "type": kind, "account_id": ACCOUNT, "device_id": DEVICE});
    if kind == "enroll" {
        message["key"] = json!(key());
    }
    serde_json::to_vec(&message).unwrap()
}

fn reply(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn enrols_then_unlocks_with_one_prompt_and_forgets() {
    let (hello, store) = (FakeHello::default(), Memory::default());
    let handler = Handler::new(callers(), &hello, &store);
    assert_eq!(
        reply(&handler.handle(&request("enroll"), Some(CHROME))),
        json!({"v": 1, "ok": true})
    );
    // Turning it on: the key is made, then two signatures are compared.
    assert_eq!(hello.state.lock().unwrap().prompts, 3);
    let record = store.0.borrow().values().next().unwrap().clone();
    assert!(!record.windows(32).any(|w| w == [0x5A; 32]), "K on disk");

    let unlocked = reply(&handler.handle(&request("unlock"), Some(CHROME)));
    assert_eq!(unlocked, json!({"v": 1, "ok": true, "key": key()}));
    assert_eq!(hello.state.lock().unwrap().prompts, 4);

    assert_eq!(
        reply(&handler.handle(&request("forget"), Some(CHROME))),
        json!({"v": 1, "ok": true})
    );
    assert!(store.0.borrow().is_empty());
    assert!(hello.state.lock().unwrap().keys.is_empty());
    assert_eq!(
        reply(&handler.handle(&request("unlock"), Some(CHROME)))["error"],
        "unknown"
    );
}

#[test]
fn only_our_extensions_may_ask() {
    let (hello, store) = (FakeHello::default(), Memory::default());
    let handler = Handler::new(callers(), &hello, &store);
    for caller in [
        None,
        Some("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"),
        Some("other@example.com"),
    ] {
        assert_eq!(
            reply(&handler.handle(&request("enroll"), caller))["error"],
            "refused"
        );
    }
    assert_eq!(hello.state.lock().unwrap().prompts, 0);
}

#[test]
fn each_caller_has_its_own_enrolment() {
    let (hello, store) = (FakeHello::default(), Memory::default());
    let handler = Handler::new(callers(), &hello, &store);
    handler.handle(&request("enroll"), Some(CHROME));
    assert_eq!(
        reply(&handler.handle(&request("unlock"), Some(FIREFOX)))["error"],
        "unknown"
    );
    assert_ne!(
        enrolment(CHROME, ACCOUNT, DEVICE),
        enrolment(FIREFOX, ACCOUNT, DEVICE)
    );
    // The name hides the caller, account and device.
    let id = enrolment(CHROME, ACCOUNT, DEVICE);
    assert_eq!(id.len(), 32);
    assert!(!id.contains("a1a1"));
}

#[test]
fn a_cancel_keeps_it_and_a_changed_key_turns_it_off() {
    let (hello, store) = (FakeHello::default(), Memory::default());
    let handler = Handler::new(callers(), &hello, &store);
    handler.handle(&request("enroll"), Some(CHROME));

    hello.state.lock().unwrap().cancel_next = true;
    assert_eq!(
        reply(&handler.handle(&request("unlock"), Some(CHROME)))["error"],
        "cancelled"
    );
    assert_eq!(store.0.borrow().len(), 1);

    // A Windows Hello reset makes a new key: its signature doesn't open K.
    let name = credential_name(&enrolment(CHROME, ACCOUNT, DEVICE));
    hello.create(&name).unwrap();
    assert_eq!(
        reply(&handler.handle(&request("unlock"), Some(CHROME)))["error"],
        "changed"
    );
    assert!(store.0.borrow().is_empty());
}

#[test]
fn turning_it_on_fails_cleanly() {
    let (hello, store) = (FakeHello::default(), Memory::default());
    let handler = Handler::new(callers(), &hello, &store);
    hello.state.lock().unwrap().unsupported = true;
    assert_eq!(
        reply(&handler.handle(&request("enroll"), Some(CHROME)))["error"],
        "unavailable"
    );
    hello.state.lock().unwrap().unsupported = false;

    hello.state.lock().unwrap().cancel_next = true;
    assert_eq!(
        reply(&handler.handle(&request("enroll"), Some(CHROME)))["error"],
        "cancelled"
    );

    // Signatures that differ each time can't be relied on (D68 §2).
    hello.state.lock().unwrap().nondeterministic = true;
    assert_eq!(
        reply(&handler.handle(&request("enroll"), Some(CHROME)))["error"],
        "unavailable"
    );
    assert!(store.0.borrow().is_empty());
    assert!(hello.state.lock().unwrap().keys.is_empty());
}

#[test]
fn malformed_requests_are_invalid() {
    let (hello, store) = (FakeHello::default(), Memory::default());
    let handler = Handler::new(callers(), &hello, &store);
    let bad = [
        b"not json".to_vec(),
        serde_json::to_vec(&json!({"v": 2, "type": "unlock", "account_id": ACCOUNT, "device_id": DEVICE})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "type": "unlock", "account_id": "A1", "device_id": DEVICE})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "type": "unlock", "account_id": ACCOUNT.to_uppercase(), "device_id": DEVICE})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "type": "enroll", "account_id": ACCOUNT, "device_id": DEVICE, "key": "c2hvcnQ="})).unwrap(),
        serde_json::to_vec(&json!({"v": 1, "type": "export", "account_id": ACCOUNT, "device_id": DEVICE})).unwrap(),
    ];
    for message in bad {
        assert_eq!(
            reply(&handler.handle(&message, Some(CHROME)))["error"],
            "invalid"
        );
    }
}

#[test]
fn a_damaged_record_is_forgotten() {
    let (hello, store) = (FakeHello::default(), Memory::default());
    let handler = Handler::new(callers(), &hello, &store);
    handler.handle(&request("enroll"), Some(CHROME));
    let id = enrolment(CHROME, ACCOUNT, DEVICE);
    store.0.borrow_mut().insert(id, vec![1, 2, 3]);
    assert_eq!(
        reply(&handler.handle(&request("unlock"), Some(CHROME)))["error"],
        "changed"
    );
    assert!(store.0.borrow().is_empty());
}

#[test]
fn the_caller_comes_from_the_arguments() {
    let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        caller(&args(&["helper.exe", CHROME, "--parent-window=0"])).as_deref(),
        Some(CHROME)
    );
    assert_eq!(
        caller(&args(&["helper.exe", "C:\\m\\host.json", FIREFOX])).as_deref(),
        Some(FIREFOX)
    );
    assert_eq!(caller(&args(&["helper.exe"])), None);
}

#[test]
fn the_manifests_name_the_helper_and_only_our_extensions() {
    let chromium: Value =
        serde_json::from_str(&manifest(false, r"C:\PWM\pwm-browser-helper.exe")).unwrap();
    assert_eq!(chromium["name"], HOST_NAME);
    assert_eq!(chromium["type"], "stdio");
    assert_eq!(chromium["path"], r"C:\PWM\pwm-browser-helper.exe");
    assert_eq!(chromium["allowed_origins"], json!([CHROME]));
    assert!(chromium.get("allowed_extensions").is_none());
    let firefox: Value = serde_json::from_str(&manifest(true, "x")).unwrap();
    assert_eq!(firefox["allowed_extensions"], json!([FIREFOX]));
    for (key, _) in REGISTRY_KEYS {
        assert!(key.ends_with(HOST_NAME) && key.starts_with(r"Software\"));
    }
}

#[test]
fn the_framing_round_trips_and_refuses_large_messages() {
    let mut buffer = Vec::new();
    write_message(&mut buffer, br#"{"v":1}"#).unwrap();
    assert_eq!(&buffer[..4], &7u32.to_ne_bytes());
    assert_eq!(
        read_message(buffer.as_slice()).unwrap().unwrap(),
        br#"{"v":1}"#
    );
    assert_eq!(read_message(&[][..]).unwrap(), None);
    let mut large = u32::try_from(MAX_MESSAGE + 1)
        .unwrap()
        .to_ne_bytes()
        .to_vec();
    large.extend(vec![b' '; 16]);
    assert!(read_message(large.as_slice()).is_err());
}
