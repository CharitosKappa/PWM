//! `PolkitKey` against the real key holder's code over a temporary folder,
//! with `pkexec`'s answers for a dismissed dialog and a refusal (D71 §8).
//! CI also runs `the_real_pkexec_answers_the_same_twice` through the real
//! `pkexec` and an installed key holder.

use std::sync::Mutex;

use super::*;

const NAME: &str = "PWM 01234567-89ab-cdef-0123-456789abcdef";

/// The key holder run in-process for user 1000, or a fixed exit code.
struct Fake {
    holder: pwm_keyholder::Holder,
    dir: std::path::PathBuf,
    next: Mutex<Option<i32>>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Fake {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pwm-polkit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self {
            holder: pwm_keyholder::Holder::at(dir.join("keys")),
            dir,
            next: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Pkexec for Fake {
    fn available(&self) -> bool {
        true
    }

    fn run(&self, args: &[&str], input: &[u8]) -> Option<Ran> {
        self.calls
            .lock()
            .unwrap()
            .push(args.iter().map(|a| (*a).to_owned()).collect());
        if let Some(code) = self.next.lock().unwrap().take() {
            return Some(Ran {
                code: Some(code),
                stdout: Vec::new(),
            });
        }
        let mut argv = vec!["pwm-keyholder".to_owned()];
        argv.extend(args.iter().map(|a| (*a).to_owned()));
        let Some(command) = pwm_keyholder::parse(&argv) else {
            return Some(Ran {
                code: Some(3),
                stdout: Vec::new(),
            });
        };
        let mut stdout = Vec::new();
        let code = match pwm_keyholder::run(&self.holder, 1000, &command, input, &mut stdout) {
            Ok(()) => 0,
            Err(error) => i32::from(error.exit_code()),
        };
        Some(Ran {
            code: Some(code),
            stdout,
        })
    }
}

#[test]
fn it_signs_the_same_challenge_the_same_way_with_linuxs_labels() {
    let key = PolkitKey(Fake::new("same"));
    assert_eq!(key.signer(), Signer::LinuxPolkit);
    assert_eq!(
        key.sign(NAME, &[1; 32]).unwrap_err(),
        HelloError::Changed,
        "no secret yet"
    );
    key.create(NAME).unwrap();
    let a = key.sign(NAME, &[1; 32]).unwrap();
    let b = key.sign(NAME, &[1; 32]).unwrap();
    assert_eq!(a.expose_secret().len(), 32);
    assert_eq!(a.expose_secret(), b.expose_secret());
    assert_ne!(
        a.expose_secret(),
        key.sign(NAME, &[2; 32]).unwrap().expose_secret()
    );
    // The key holder's name and the challenge in hex.
    let calls = key.0.calls.lock().unwrap().clone();
    assert_eq!(
        calls[1],
        ["create", "pwm-01234567-89ab-cdef-0123-456789abcdef"]
    );
}

#[test]
fn a_dismissed_dialog_cancels_and_a_refusal_is_unavailable() {
    let key = PolkitKey(Fake::new("dismissed"));
    *key.0.next.lock().unwrap() = Some(126);
    assert_eq!(key.create(NAME).unwrap_err(), HelloError::Cancelled);
    *key.0.next.lock().unwrap() = Some(127);
    assert_eq!(key.create(NAME).unwrap_err(), HelloError::Unavailable);
    key.create(NAME).unwrap();
    *key.0.next.lock().unwrap() = Some(126);
    assert_eq!(key.sign(NAME, &[1; 32]).unwrap_err(), HelloError::Cancelled);
}

#[test]
fn names_and_challenges_are_checked_before_asking() {
    let key = PolkitKey(Fake::new("checked"));
    assert_eq!(key.create("PWM/../x").unwrap_err(), HelloError::Unavailable);
    assert_eq!(
        key.sign(NAME, &[1; 31]).unwrap_err(),
        HelloError::Unavailable
    );
    assert!(key.0.calls.lock().unwrap().is_empty());
    assert_eq!(
        holder_name("PWM browser 0123456789abcdef0123456789abcdef").unwrap(),
        "pwm-browser-0123456789abcdef0123456789abcdef"
    );
}

/// Through the real `pkexec` and an installed key holder, with a polkit
/// rule that allows this user without a dialog: CI's `desktop-linux` job
/// (`PWM_TEST_PKEXEC=1`).
#[test]
fn the_real_pkexec_answers_the_same_twice() {
    if std::env::var_os("PWM_TEST_PKEXEC").is_none() {
        return;
    }
    let key = PolkitKey::system();
    assert!(key.supported());
    let name = "PWM test 0123456789abcdef";
    key.create(name).unwrap();
    let a = key.sign(name, &[7; 32]).unwrap();
    let b = key.sign(name, &[7; 32]).unwrap();
    assert_eq!(a.expose_secret(), b.expose_secret());
    assert_ne!(
        a.expose_secret(),
        key.sign(name, &[8; 32]).unwrap().expose_secret()
    );
    assert_eq!(
        key.sign("PWM test missing", &[7; 32]).unwrap_err(),
        HelloError::Changed
    );
}
