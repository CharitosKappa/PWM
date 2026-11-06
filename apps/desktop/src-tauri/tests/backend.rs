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

/// The server allows 5 sign-ups an hour from one address, and CI's
/// end-to-end check needs one: these steps share 3 accounts, in order.
#[tokio::test]
async fn against_a_server() {
    let Some(server) = server() else { return };
    let (mut a, email, secret_key) = sign_up_lock_and_unlock(&server, true).await;
    sign_up_lock_and_unlock(&server, false).await;
    unlock(&mut a).await;
    import_health_and_export(&mut a).await;
    unlock(&mut a).await;
    devices_and_approving_a_new_one(&server, &mut a, &email).await;
    sign_in_on_another_computer_and_sign_out(&server, &email, &secret_key).await;
}

async fn unlock(r: &mut Rig) {
    r.ok(json!({"kind": "unlock", "password": PASSWORD})).await;
}

/// Signs up (secure: a system key store; otherwise the tokens sealed by
/// the core), locks and unlocks, and leaves it locked by the timeout.
async fn sign_up_lock_and_unlock(server: &str, secure: bool) -> (Rig, String, String) {
    {
        let (mut r, email, secret_key) = signed_up(server, secure).await;
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
        (r, email, secret_key)
    }
}

async fn sign_in_on_another_computer_and_sign_out(server: &str, email: &str, secret_key: &str) {
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

    // Another account on this computer gets a device id of its own: the
    // server ties one to a single account (D59's notes).
    let other = unique_email();
    let reply = second
        .ok(json!({"kind": "signUp", "email": other, "password": PASSWORD}))
        .await;
    assert!(reply["secretKey"].is_string());
    let new_id = second.keys.get(name::DEVICE_ID).await.unwrap().unwrap();
    assert_ne!(new_id.expose_secret(), device_id.expose_secret());
    second.ok(json!({"kind": "signOut"})).await;
    // Signing in to that account again reuses its id.
    second
        .ok(json!({"kind": "signIn", "email": other, "password": PASSWORD, "secretKey": reply["secretKey"]}))
        .await;
    let again = second.keys.get(name::DEVICE_ID).await.unwrap().unwrap();
    assert_eq!(again.expose_secret(), new_id.expose_secret());
}

fn base64_decode(text: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap()
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

async fn import_health_and_export(r: &mut Rig) {
    // A CSV with two logins sharing a weak password.
    let csv = "name,url,username,password\n\
               Mail,https://mail.example.com,a@example.com,hunter2\n\
               Bank,https://bank.example.com,b@example.com,hunter2\n";
    let file = base64_encode(csv.as_bytes());
    let detected = r
        .ok(json!({"kind": "importDetect", "name": "export.csv", "file": file}))
        .await;
    assert_eq!(detected["format"], "csv", "{detected}");
    assert_eq!(detected["columns"]["rows"], 2, "{detected}");
    let columns = detected["columns"]["suggested"].clone();
    let preview = r
        .ok(json!({"kind": "importPrepare", "file": file, "format": "csv", "password": "", "keyFile": "", "columns": columns}))
        .await;
    assert_eq!(preview["logins"], 2, "{preview}");
    let (done, after) = r
        .backend
        .handle(request(json!({"kind": "importFinish"})))
        .await;
    assert_eq!(done.unwrap()["logins"], 2);
    assert_eq!(after, After::Sync);
    let items = r.ok(json!({"kind": "items", "query": ""})).await;
    assert_eq!(items.as_array().unwrap().len(), 2, "{items}");

    // Health: both weak, one reused group; no leak results yet.
    let health = r.ok(json!({"kind": "health"})).await;
    assert_eq!(health["weak"].as_array().unwrap().len(), 2, "{health}");
    assert_eq!(health["reused"].as_array().unwrap().len(), 1, "{health}");
    assert_eq!(health["reused"][0][0]["title"], "Bank", "{health}");
    assert_eq!(health["leaked"], Value::Null);
    assert_eq!(health["leakCheck"], true);
    // Never a password in the report.
    assert!(!health.to_string().contains("hunter2"));
    assert_eq!(
        r.ok(json!({"kind": "setLeakCheck", "on": false})).await,
        json!(false)
    );
    assert_eq!(
        r.ask(json!({"kind": "checkLeaks"})).await,
        Err("leak_check_off".to_owned())
    );

    // Export: the master password first, a strong export password for .pwmx.
    assert_eq!(
        r.ask(json!({"kind": "exportVault", "masterPassword": "not it at all", "format": "json", "exportPassword": ""}))
            .await,
        Err("wrong_password".to_owned())
    );
    assert_eq!(
        r.ask(json!({"kind": "exportVault", "masterPassword": PASSWORD, "format": "pwmx", "exportPassword": "short"}))
            .await,
        Err("weak_export_password".to_owned())
    );
    let export = r
        .ok(json!({"kind": "exportVault", "masterPassword": PASSWORD, "format": "json", "exportPassword": ""}))
        .await;
    let name = export["name"].as_str().unwrap();
    assert!(
        name.starts_with("PWM export ") && name.ends_with(".json"),
        "{name}"
    );
    let text = String::from_utf8(base64_decode(export["file"].as_str().unwrap())).unwrap();
    assert!(text.contains("mail.example.com"));

    // Locking forgets the import and the leak results.
    r.ok(json!({"kind": "lock"})).await;
    assert_eq!(
        r.ask(json!({"kind": "health"})).await,
        Err("locked".to_owned())
    );
}

async fn devices_and_approving_a_new_one(server: &str, first: &mut Rig, email: &str) {
    // The new computer asks; the signed-in one sees it after a sync.
    let mut second = rig(true);
    second
        .ok(json!({"kind": "setSettings", "settings": {"serverUrl": server, "autoLockMinutes": 15}}))
        .await;
    let asked = second
        .ok(json!({"kind": "askApproval", "email": email}))
        .await;
    assert_eq!(asked["state"], "waiting", "{asked}");
    let status = first.ok(json!({"kind": "sync"})).await;
    assert_eq!(status["approvals"], 1, "{status}");
    let list = first.ok(json!({"kind": "approvals"})).await;
    let id = list[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(list[0]["platform"], "windows");

    // Accept, compare the codes, complete: the Secret Key arrives.
    first.ok(json!({"kind": "acceptApproval", "id": id})).await;
    let polled = second.ok(json!({"kind": "askApprovalPoll"})).await;
    assert_eq!(polled["state"], "code", "{polled}");
    let list = first.ok(json!({"kind": "approvals"})).await;
    assert_eq!(list[0]["code"], polled["code"], "{list}");
    first
        .ok(json!({"kind": "completeApproval", "id": id}))
        .await;
    let polled = second.ok(json!({"kind": "askApprovalPoll"})).await;
    assert_eq!(polled["state"], "approved", "{polled}");
    second
        .ok(json!({"kind": "signInApproved", "password": PASSWORD}))
        .await;
    assert_eq!(
        second.ok(json!({"kind": "status"})).await["state"],
        "unlocked"
    );

    // Devices: rename this one; this one can't be revoked, the other can.
    let devices = first.ok(json!({"kind": "devices"})).await;
    assert_eq!(devices.as_array().unwrap().len(), 2, "{devices}");
    let this = devices
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["current"] == true)
        .unwrap()["id"]
        .clone();
    let other = devices
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["current"] == false)
        .unwrap()["id"]
        .clone();
    let renamed = first
        .ok(json!({"kind": "renameDevice", "deviceId": this, "name": "  Desk 2  "}))
        .await;
    assert!(renamed.to_string().contains("\"Desk 2\""), "{renamed}");
    assert_eq!(
        first
            .ask(json!({"kind": "renameDevice", "deviceId": this, "name": "  "}))
            .await,
        Err("invalid_device_name".to_owned())
    );
    assert_eq!(
        first
            .ask(json!({"kind": "revokeDevice", "deviceId": this}))
            .await,
        Err("this_device".to_owned())
    );
    let after = first
        .ok(json!({"kind": "revokeDevice", "deviceId": other}))
        .await;
    assert!(
        after
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == other && d["revokedAt"].is_number())
    );

    // The revoked computer finds out at its next sync and keeps nothing.
    let status = second.ok(json!({"kind": "sync"})).await;
    assert_eq!(status, json!({"state": "signedOut"}), "{status}");
    assert!(second.keys.get(name::SECRET_KEY).await.unwrap().is_none());
}
