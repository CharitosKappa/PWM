//! Registering with the browsers and undoing it, against a temporary folder,
//! a registry in memory and a fake Windows Hello (D69 §4, D70 §5).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pwm_hello::FakeHello;

use super::hosts::*;
use super::{HelloKey, Store, StoreError, credential_name};

const EDGE: &str = r"Software\Microsoft\Edge\NativeMessagingHosts\invalid.pwm.browser";
const FIREFOX: &str = r"Software\Mozilla\NativeMessagingHosts\invalid.pwm.browser";

#[derive(Default, Clone)]
struct FakeRegistry(Arc<Mutex<HashMap<String, String>>>);

impl Registry for FakeRegistry {
    fn get(&self, path: &str) -> Option<String> {
        self.0.lock().unwrap().get(path).cloned()
    }
    fn set(&self, path: &str, value: &str) -> Result<(), RegistryError> {
        self.0
            .lock()
            .unwrap()
            .insert(path.to_owned(), value.to_owned());
        Ok(())
    }
    fn remove(&self, path: &str) {
        self.0.lock().unwrap().remove(path);
    }
}

#[derive(Default, Clone)]
struct FakeEnrolments(Arc<Mutex<HashMap<String, Vec<u8>>>>);

impl Store for FakeEnrolments {
    fn get(&self, id: &str) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(id).cloned()
    }
    fn set(&self, id: &str, record: &[u8]) -> Result<(), StoreError> {
        self.0
            .lock()
            .unwrap()
            .insert(id.to_owned(), record.to_vec());
        Ok(())
    }
    fn delete(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
    fn ids(&self) -> Vec<String> {
        self.0.lock().unwrap().keys().cloned().collect()
    }
}

struct Rig {
    hosts: BrowserHosts,
    registry: FakeRegistry,
    enrolments: FakeEnrolments,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Hosts over a temporary folder with a stand-in helper file.
fn rig(name: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("pwm-hosts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let helper = dir.join("pwm-browser-helper.exe");
    std::fs::write(&helper, b"").unwrap();
    let (registry, enrolments) = (FakeRegistry::default(), FakeEnrolments::default());
    let hosts = BrowserHosts::new(
        dir.join("NativeMessagingHosts"),
        helper,
        Box::new(registry.clone()),
        Box::new(enrolments.clone()),
    );
    Rig {
        hosts,
        registry,
        enrolments,
        dir,
    }
}

#[test]
fn turning_on_writes_manifests_and_keys_for_each_browser() {
    let r = rig("on");
    assert!(!r.hosts.is_on());
    r.hosts.turn_on().unwrap();
    assert!(r.hosts.is_on());
    let keys = r.registry.0.lock().unwrap().clone();
    assert_eq!(keys.len(), 3);
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&keys[FIREFOX]).unwrap()).unwrap();
    assert_eq!(manifest["allowed_extensions"][0], "pwm@pwm.invalid");
    assert_eq!(
        manifest["path"],
        r.dir.join("pwm-browser-helper.exe").to_str().unwrap()
    );
}

#[test]
fn turning_off_removes_ours_and_forgets_every_enrolment() {
    let r = rig("off");
    r.hosts.turn_on().unwrap();
    // Another program's key with our name stays.
    r.registry
        .0
        .lock()
        .unwrap()
        .insert(EDGE.to_owned(), r"C:\Other\manifest.json".to_owned());
    let hello = FakeHello::default();
    let id = "0123456789abcdef0123456789abcdef";
    hello.create(&credential_name(id)).unwrap();
    r.enrolments.set(id, b"record").unwrap();

    r.hosts.turn_off(&hello);
    assert!(!r.hosts.is_on());
    assert_eq!(r.registry.0.lock().unwrap().len(), 1);
    assert!(r.enrolments.ids().is_empty());
    assert!(hello.state.lock().unwrap().keys.is_empty());
    // The manifests' folder goes once it's empty.
    assert!(!r.dir.join("NativeMessagingHosts").exists());
}

#[test]
fn turning_off_keeps_a_folder_with_anything_else_in_it() {
    let r = rig("keep");
    r.hosts.turn_on().unwrap();
    let other = r.dir.join("NativeMessagingHosts").join("other.json");
    std::fs::write(&other, b"{}").unwrap();
    r.hosts.turn_off(&FakeHello::default());
    assert!(other.exists());
    assert!(
        !r.dir
            .join("NativeMessagingHosts")
            .join("invalid.pwm.browser.chromium.json")
            .exists()
    );
}

#[test]
fn a_missing_helper_is_said_plainly() {
    let r = rig("missing");
    std::fs::remove_file(r.dir.join("pwm-browser-helper.exe")).unwrap();
    assert_eq!(r.hosts.turn_on().unwrap_err(), HostsError::HelperMissing);
    assert!(r.registry.0.lock().unwrap().is_empty());
}

#[test]
fn only_the_uninstaller_asks_for_uninstall() {
    let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    assert!(is_uninstall(&args(&["helper.exe", "--uninstall"])));
    // What the browsers pass.
    assert!(!is_uninstall(&args(&[
        "helper.exe",
        "chrome-extension://pppppppppppppppppppppppppppppppp/",
    ])));
    assert!(!is_uninstall(&args(&[
        "helper.exe",
        r"C:\PWM\invalid.pwm.browser.firefox.json",
        "pwm@pwm.invalid",
    ])));
    assert!(!is_uninstall(&args(&["helper.exe", "--uninstall", "x"])));
    assert!(!is_uninstall(&args(&["helper.exe"])));
}
