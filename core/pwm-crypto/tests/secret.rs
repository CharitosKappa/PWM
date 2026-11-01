use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pwm_crypto::Secret;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Records whether `zeroize` ran, so drop behaviour can be observed without
/// reading freed memory.
struct Probe {
    zeroized: Arc<AtomicBool>,
}

impl Zeroize for Probe {
    fn zeroize(&mut self) {
        self.zeroized.store(true, Ordering::SeqCst);
    }
}

#[test]
fn zeroizes_on_drop() {
    let flag = Arc::new(AtomicBool::new(false));
    let secret = Secret::new(Probe {
        zeroized: Arc::clone(&flag),
    });
    assert!(!flag.load(Ordering::SeqCst));
    drop(secret);
    assert!(flag.load(Ordering::SeqCst));
}

#[test]
fn debug_is_redacted() {
    let secret = Secret::new(String::from("hunter2"));
    let out = format!("{secret:?} {secret:#?}");
    assert!(!out.contains("hunter2"), "{out}");
    assert!(out.contains("REDACTED"), "{out}");
}

#[test]
fn display_is_redacted() {
    let secret = Secret::new(String::from("hunter2"));
    let out = secret.to_string();
    assert!(!out.contains("hunter2"), "{out}");
    assert!(out.contains("REDACTED"), "{out}");
}

#[test]
fn redaction_holds_inside_containing_structs() {
    #[derive(Debug)]
    #[allow(dead_code)]
    struct Login {
        username: &'static str,
        password: Secret<String>,
    }
    let login = Login {
        username: "alice",
        password: Secret::new(String::from("hunter2")),
    };
    let out = format!("{login:?}");
    assert!(out.contains("alice"));
    assert!(!out.contains("hunter2"), "{out}");
}

#[test]
fn expose_gives_access_to_the_value() {
    let mut secret = Secret::new(vec![1u8, 2, 3]);
    assert_eq!(secret.expose_secret(), &[1, 2, 3]);
    secret.expose_secret_mut().push(4);
    assert_eq!(secret.expose_secret(), &[1, 2, 3, 4]);
}

#[test]
fn new_with_fills_in_place() {
    let key = Secret::<[u8; 32]>::new_with(|k| k.fill(0xAB));
    assert_eq!(key.expose_secret(), &[0xAB; 32]);
}

#[test]
fn from_value() {
    let secret: Secret<String> = String::from("pw").into();
    assert_eq!(secret.expose_secret(), "pw");
}

#[test]
fn is_zeroize_on_drop() {
    fn assert_zod<T: ZeroizeOnDrop>() {}
    assert_zod::<Secret<[u8; 32]>>();
}

#[test]
fn is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Secret<[u8; 32]>>();
}
