//! The Linux manifests against a temporary home (D71 §4, §8).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pwm_hello::FakeHello;

use super::hosts::{Hosts, HostsError};
use super::manifest_hosts::*;
use super::{Store, StoreError};

#[derive(Default, Clone)]
struct Enrolments(Arc<Mutex<HashMap<String, Vec<u8>>>>);

impl Store for Enrolments {
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

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn home(name: &str, browsers: &[&str]) -> (ManifestHosts, Enrolments, Home) {
    let dir = std::env::temp_dir().join(format!("pwm-home-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for browser in browsers {
        std::fs::create_dir_all(dir.join(browser)).unwrap();
    }
    let helper = dir.join("usr-bin").join("pwm-browser-helper");
    std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
    std::fs::write(&helper, b"").unwrap();
    let enrolments = Enrolments::default();
    let hosts = ManifestHosts::new(dir.clone(), helper, Box::new(enrolments.clone()));
    (hosts, enrolments, Home(dir))
}

#[test]
fn turning_on_writes_a_manifest_for_each_installed_browser() {
    let (hosts, _, home) = home("on", &[".config/google-chrome", ".mozilla"]);
    assert!(!hosts.is_on());
    hosts.turn_on().unwrap();
    assert!(hosts.is_on());
    let chrome = home
        .0
        .join(".config/google-chrome/NativeMessagingHosts/invalid.pwm.browser.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&chrome).unwrap()).unwrap();
    assert_eq!(
        manifest["allowed_origins"][0],
        "chrome-extension://pppppppppppppppppppppppppppppppp/"
    );
    assert_eq!(
        manifest["path"],
        home.0
            .join("usr-bin")
            .join("pwm-browser-helper")
            .to_str()
            .unwrap()
    );
    let firefox = home
        .0
        .join(".mozilla/native-messaging-hosts/invalid.pwm.browser.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&firefox).unwrap()).unwrap();
    assert_eq!(manifest["allowed_extensions"][0], "pwm@pwm.invalid");
    // A browser that isn't installed gets nothing.
    assert!(!home.0.join(".config/microsoft-edge").exists());
}

#[test]
fn without_a_browser_it_says_so() {
    let (hosts, _, _home) = home("none", &[]);
    assert_eq!(hosts.turn_on().unwrap_err(), HostsError::NoBrowser);
    assert!(!hosts.is_on());
}

#[test]
fn turning_off_removes_ours_and_forgets_every_enrolment() {
    let (hosts, enrolments, home) = home("off", &[".config/chromium", ".config/vivaldi"]);
    hosts.turn_on().unwrap();
    // Another program's manifest with our name, naming its own helper, stays.
    let vivaldi = home
        .0
        .join(".config/vivaldi/NativeMessagingHosts/invalid.pwm.browser.json");
    std::fs::write(
        &vivaldi,
        r#"{"name":"invalid.pwm.browser","path":"/opt/other"}"#,
    )
    .unwrap();
    enrolments
        .set("0123456789abcdef0123456789abcdef", b"record")
        .unwrap();
    hosts.turn_off(&FakeHello::default());
    assert!(!hosts.is_on());
    assert!(
        !home
            .0
            .join(".config/chromium/NativeMessagingHosts/invalid.pwm.browser.json")
            .exists()
    );
    assert!(vivaldi.exists());
    assert!(enrolments.ids().is_empty());
}

#[test]
fn a_missing_helper_is_said_plainly() {
    let (hosts, _, home) = home("missing", &[".mozilla"]);
    std::fs::remove_file(home.0.join("usr-bin/pwm-browser-helper")).unwrap();
    assert_eq!(hosts.turn_on().unwrap_err(), HostsError::HelperMissing);
}
