//! The backend's answers (D66 §3) with fake key stores, a fake clipboard and
//! a clock the tests move. The server tests sign up, lock, unlock, sync and
//! sign in again on a real server: they run when `PWM_TEST_SERVER` is set
//! (`just server`; CI's `desktop` job), and are skipped otherwise.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use pwm_client::Platform;
use pwm_desktop::backend::{After, Backend, Clock, Device};
use pwm_desktop::clipboard::{Clipboard, ClipboardError, MemoryClipboard};
use pwm_desktop::http::Reqwest;
use pwm_desktop::keys::{KeyStore, MemoryKeys, name};
use pwm_desktop::protocol::Request;
use serde_json::{Value, json};

#[derive(Default)]
struct TestClock {
    wall: AtomicI64,
    mono: AtomicU64,
}

#[derive(Clone, Default)]
struct Clocks(Arc<TestClock>);

impl Clocks {
    fn advance(&self, ms: u64) {
        self.0.mono.fetch_add(ms, Ordering::SeqCst);
        self.0
            .wall
            .fetch_add(i64::try_from(ms).unwrap(), Ordering::SeqCst);
    }
}

impl Clock for Clocks {
    fn now_ms(&self) -> i64 {
        1_762_400_000_000 + self.0.wall.load(Ordering::SeqCst)
    }
    fn mono_ms(&self) -> u64 {
        self.0.mono.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Default)]
struct Board(Arc<MemoryClipboard>);

impl Board {
    fn text(&self) -> Option<String> {
        self.0.text.lock().unwrap().clone()
    }
}

impl Clipboard for Board {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        self.0.write(text)
    }
    fn clear(&self) -> Result<(), ClipboardError> {
        self.0.clear()
    }
}

#[derive(Clone, Default)]
struct Keys(Arc<MemoryKeys>);

impl KeyStore for Keys {
    fn secure(&self) -> bool {
        self.0.secure()
    }
    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<pwm_client::Secret<String>>, pwm_desktop::keys::KeyError> {
        self.0.get(key).await
    }
    async fn set(&self, key: &str, value: &str) -> Result<(), pwm_desktop::keys::KeyError> {
        self.0.set(key, value).await
    }
    async fn remove(&self, key: &str) -> Result<(), pwm_desktop::keys::KeyError> {
        self.0.remove(key).await
    }
}

type TestBackend = Backend<Reqwest, Keys, Board, Clocks>;

struct Rig {
    backend: TestBackend,
    keys: Keys,
    board: Board,
    clocks: Clocks,
}

fn rig(secure: bool) -> Rig {
    let keys = Keys(Arc::new(if secure {
        MemoryKeys::default()
    } else {
        MemoryKeys::insecure()
    }));
    let board = Board::default();
    let clocks = Clocks::default();
    let backend = Backend::new(
        pwm_store::SqliteStore::open_in_memory().unwrap(),
        Reqwest::new().unwrap(),
        keys.clone(),
        board.clone(),
        clocks.clone(),
        Device {
            platform: Platform::Windows,
            name: "Test desk".to_owned(),
        },
        None,
    );
    Rig {
        backend,
        keys,
        board,
        clocks,
    }
}

fn request(json: Value) -> Request {
    serde_json::from_value(json).unwrap()
}

impl Rig {
    async fn ask(&mut self, json: Value) -> Result<Value, String> {
        self.backend.handle(request(json)).await.0.map_err(|f| f.0)
    }

    async fn ok(&mut self, json: Value) -> Value {
        let kind = json["kind"].clone();
        self.ask(json)
            .await
            .unwrap_or_else(|e| panic!("{kind}: {e}"))
    }
}

#[tokio::test]
async fn signed_out_it_answers_what_needs_no_account() {
    let mut r = rig(true);
    assert_eq!(
        r.ok(json!({"kind": "status"})).await,
        json!({"state": "signedOut"})
    );
    assert_eq!(
        r.ok(json!({"kind": "settings"})).await,
        json!({"serverUrl": "https://api.pwm.invalid", "autoLockMinutes": 15})
    );
    let password = r.ok(json!({"kind": "generate", "mode": "password"})).await;
    assert_eq!(password.as_str().unwrap().chars().count(), 20);
    let phrase = r
        .ok(json!({"kind": "generate", "mode": "passphrase"}))
        .await;
    assert_eq!(phrase.as_str().unwrap().split('-').count(), 5);
    let weak = r
        .ok(json!({"kind": "score", "password": "password", "email": "a@example.com"}))
        .await;
    assert_eq!(weak, json!(0));
    // The browser's requests.
    assert_eq!(
        r.ask(json!({"kind": "suggestions"})).await,
        Err("unsupported".to_owned())
    );
    assert_eq!(
        r.ask(json!({"kind": "copySecretKey"})).await,
        Err("signed_out".to_owned())
    );
    assert_eq!(
        r.ask(json!({"kind": "unlock", "password": "x"})).await,
        Err("signed_out".to_owned())
    );
}

#[tokio::test]
async fn settings_are_checked_like_the_extensions() {
    let mut r = rig(true);
    let set = |url: &str, minutes: u32| json!({"kind": "setSettings", "settings": {"serverUrl": url, "autoLockMinutes": minutes}});
    assert_eq!(
        r.ok(set("http://localhost:8080/", 5)).await,
        json!({"serverUrl": "http://localhost:8080", "autoLockMinutes": 5})
    );
    assert_eq!(
        r.ask(set("http://example.com", 5)).await,
        Err("invalid_server".to_owned())
    );
    assert_eq!(
        r.ask(set("https://example.com", 7)).await,
        Err("invalid_auto_lock".to_owned())
    );
}

#[tokio::test]
async fn a_copy_is_cleared_thirty_seconds_later() {
    let mut r = rig(true);
    assert_eq!(
        r.ask(json!({"kind": "copyText", "text": ""})).await,
        Err("empty".to_owned())
    );
    r.ok(json!({"kind": "copyText", "text": "s3cret-value"}))
        .await;
    assert_eq!(r.board.text().as_deref(), Some("s3cret-value"));
    r.clocks.advance(29_999);
    r.backend.tick();
    assert_eq!(r.board.text().as_deref(), Some("s3cret-value"));
    r.clocks.advance(1);
    r.backend.tick();
    assert_eq!(r.board.text(), None);
}

// --- Against a real server ---

fn server() -> Option<String> {
    std::env::var("PWM_TEST_SERVER")
        .ok()
        .filter(|s| !s.is_empty())
}

fn unique_email() -> String {
    format!("desktop-{}@example.com", pwm_client::new_id().unwrap())
}

const PASSWORD: &str = "tangerine orbit velvet canyon 42";

async fn signed_up(server: &str, secure: bool) -> (Rig, String, String) {
    let mut r = rig(secure);
    r.ok(json!({"kind": "setSettings", "settings": {"serverUrl": server, "autoLockMinutes": 1}}))
        .await;
    let email = unique_email();
    let reply = r
        .ok(json!({"kind": "signUp", "email": email, "password": PASSWORD}))
        .await;
    let secret_key = reply["secretKey"].as_str().unwrap().to_owned();
    (r, email, secret_key)
}

#[tokio::test]
async fn sign_up_lock_unlock_and_the_lock_timeout() {
    let Some(server) = server() else { return };
    for secure in [true, false] {
        let (mut r, email, secret_key) = signed_up(&server, secure).await;
        let status = r.ok(json!({"kind": "status"})).await;
        assert_eq!(status["state"], "unlocked", "{status}");
        assert_eq!(status["email"], json!(email));
        assert_eq!(status["syncError"], Value::Null, "{status}");
        assert_eq!(r.ok(json!({"kind": "items", "query": ""})).await, json!([]));

        // The kit, and the Secret Key copied.
        let kit = r.ok(json!({"kind": "emergencyKit"})).await;
        let pdf = base64_decode(kit["pdf"].as_str().unwrap());
        assert!(pdf.starts_with(b"%PDF"));
        r.ok(json!({"kind": "copySecretKey"})).await;
        assert_eq!(r.board.text().as_deref(), Some(secret_key.as_str()));

        // What is kept: tokens as they are in a secure store, sealed otherwise.
        let tokens = r.keys.get(name::TOKENS).await.unwrap();
        let sealed = r.keys.get(name::SEALED_TOKENS).await.unwrap();
        assert_eq!((tokens.is_some(), sealed.is_some()), (secure, !secure));

        // Locking, a wrong password, then the right one.
        r.ok(json!({"kind": "lock"})).await;
        assert_eq!(r.ok(json!({"kind": "status"})).await["state"], "locked");
        assert_eq!(
            r.ask(json!({"kind": "unlock", "password": "not it at all"}))
                .await,
            Err("wrong_password".to_owned())
        );
        let (reply, after) = r
            .backend
            .handle(request(json!({"kind": "unlock", "password": PASSWORD})))
            .await;
        assert!(reply.is_ok());
        assert_eq!(after, After::Sync);
        // The tokens still work after unlocking: a sync goes through.
        let status = r.ok(json!({"kind": "sync"})).await;
        assert_eq!(
            status["syncError"],
            Value::Null,
            "secure {secure}: {status}"
        );
        assert!(status["syncedAt"].is_number());

        // One minute without use locks it.
        r.clocks.advance(59_000);
        assert!(!r.backend.tick().locked);
        r.clocks.advance(1_000);
        assert!(r.backend.tick().locked);
        assert_eq!(r.ok(json!({"kind": "status"})).await["state"], "locked");
    }
}

#[tokio::test]
async fn sign_in_on_a_second_computer_and_sign_out() {
    let Some(server) = server() else { return };
    let (_first, email, secret_key) = signed_up(&server, true).await;
    let mut second = rig(true);
    second
        .ok(json!({"kind": "setSettings", "settings": {"serverUrl": server, "autoLockMinutes": 15}}))
        .await;
    assert_eq!(
        second
            .ask(json!({"kind": "signIn", "email": email, "password": "wrong password here", "secretKey": secret_key}))
            .await,
        Err("invalid_credentials".to_owned())
    );
    second
        .ok(json!({"kind": "signIn", "email": email, "password": PASSWORD, "secretKey": secret_key}))
        .await;
    assert_eq!(
        second.ok(json!({"kind": "status"})).await["state"],
        "unlocked"
    );
    let device_id = second.keys.get(name::DEVICE_ID).await.unwrap().unwrap();

    second.ok(json!({"kind": "signOut"})).await;
    assert_eq!(
        second.ok(json!({"kind": "status"})).await,
        json!({"state": "signedOut"})
    );
    for key in [name::EMAIL, name::SECRET_KEY, name::TOKENS] {
        assert!(second.keys.get(key).await.unwrap().is_none(), "{key}");
    }
    // The device id stays for this install.
    assert_eq!(
        second
            .keys
            .get(name::DEVICE_ID)
            .await
            .unwrap()
            .unwrap()
            .expose_secret(),
        device_id.expose_secret()
    );
}

fn base64_decode(text: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap()
}
