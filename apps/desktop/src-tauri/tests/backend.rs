//! The backend's answers (D66 §3) with fake key stores, a fake clipboard and
//! a clock the tests move. The server tests sign up, lock, unlock, sync and
//! sign in again on a real server: they run when `PWM_TEST_SERVER` is set
//! (`just server`; CI's `desktop` job), and are skipped otherwise.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use pwm_client::Platform;
use pwm_desktop::backend::{After, Backend, Clock, Device};
use pwm_desktop::clipboard::{Clipboard, ClipboardError, MemoryClipboard};
use pwm_desktop::hello::{FakeHello, HelloError, HelloKey};
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

/// Windows Hello as a fake the tests can steer (D68).
#[derive(Clone, Default)]
struct Hello(Arc<FakeHello>);

impl HelloKey for Hello {
    fn supported(&self) -> bool {
        self.0.supported()
    }
    fn create(&self, name: &str) -> Result<(), HelloError> {
        self.0.create(name)
    }
    fn sign(&self, name: &str, data: &[u8]) -> Result<pwm_client::Secret<Vec<u8>>, HelloError> {
        self.0.sign(name, data)
    }
    fn delete(&self, name: &str) {
        self.0.delete(name);
    }
}

struct Rig {
    backend: TestBackend,
    keys: Keys,
    board: Board,
    clocks: Clocks,
    hello: Hello,
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
    let hello = Hello::default();
    let backend = backend.with_hello(Box::new(hello.clone()));
    Rig {
        backend,
        keys,
        board,
        clocks,
        hello,
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
    // Five words and one digit, joined by "-". A few list words contain "-"
    // themselves ("t-shirt"), so a phrase can split into up to ten parts.
    let phrase = phrase.as_str().unwrap();
    let parts = phrase.split('-').count();
    assert!((5..=10).contains(&parts), "{parts} parts");
    assert_eq!(phrase.chars().filter(char::is_ascii_digit).count(), 1);
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
    items(&mut a).await;
    windows_hello(&mut a).await;
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

        // The plan (D6): the test server sells none, so every account is
        // Premium and there is nothing to buy or cancel.
        assert_eq!(
            r.ok(json!({"kind": "plan"})).await,
            json!({"plan": "premium", "subscription": null, "sells": []})
        );

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

    // Deleting that account (D74): the password first, then everything
    // goes, and the sign-in screen says why.
    assert_eq!(
        second
            .ask(json!({"kind": "deleteAccount", "masterPassword": "not it at all"}))
            .await,
        Err("wrong_password".to_owned())
    );
    assert_eq!(
        second.ok(json!({"kind": "status"})).await["state"],
        "unlocked"
    );
    assert_eq!(
        second
            .ok(json!({"kind": "deleteAccount", "masterPassword": PASSWORD}))
            .await,
        Value::Null
    );
    assert_eq!(
        second.ok(json!({"kind": "status"})).await,
        json!({"state": "signedOut", "ended": "account_deleted"})
    );
    for key in [name::EMAIL, name::SECRET_KEY, name::TOKENS] {
        assert!(second.keys.get(key).await.unwrap().is_none(), "{key}");
    }
    // It's gone on the server too.
    assert_eq!(
        second
            .ask(json!({"kind": "signIn", "email": other, "password": PASSWORD, "secretKey": reply["secretKey"]}))
            .await,
        Err("invalid_credentials".to_owned())
    );
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

    // The revoked computer finds out at its next sync, keeps nothing, and
    // the sign-in screen says why.
    let status = second.ok(json!({"kind": "sync"})).await;
    assert_eq!(
        status,
        json!({"state": "signedOut", "ended": "device_revoked"}),
        "{status}"
    );
    assert!(second.keys.get(name::SECRET_KEY).await.unwrap().is_none());
}

/// The item screens' requests (D67), on an unlocked account that already
/// holds the two imported logins.
async fn items(r: &mut Rig) {
    let rows = |v: &Value| v["items"].as_array().unwrap().clone();
    assert_eq!(rows(&r.ok(json!({"kind": "vault"})).await).len(), 2);

    // A login with a one-time code and a hidden custom field.
    let totp = r
        .ok(json!({"kind": "totpSetup", "input": "JBSWY3DPEHPK3PXP", "title": "Forum"}))
        .await;
    assert!(
        totp.as_str().unwrap().starts_with("otpauth://totp/"),
        "{totp}"
    );
    assert_eq!(
        r.ask(json!({"kind": "totpSetup", "input": "https://example.com/?", "title": "Forum"}))
            .await,
        Err("invalid_totp".to_owned())
    );
    let saved = r
        .ok(json!({"kind": "saveItem", "vaultId": null, "item": {
            "type": "login", "title": "Forum", "notes": "", "favorite": false,
            "folder_id": null, "tags": ["social"],
            "custom_fields": [{"name": "PIN", "value": "pin-7777", "kind": "hidden"}],
            "login": {"username": "me", "password": "pw-ONE-xyz",
                      "urls": [{"url": "https://forum.example.com", "match": "domain"}],
                      "totp": totp}
        }}))
        .await;
    let (vault_id, item_id) = (saved["vaultId"].clone(), saved["itemId"].clone());
    let at = |kind: &str| json!({"kind": kind, "vaultId": vault_id, "itemId": item_id});

    // The list and the detail never hold a hidden value.
    let vault = r.ok(json!({"kind": "vault"})).await;
    let shown = vault.to_string();
    assert!(!shown.contains("pw-ONE-xyz") && !shown.contains("pin-7777"));
    let row = rows(&vault)
        .into_iter()
        .find(|i| i["itemId"] == item_id)
        .unwrap();
    assert_eq!(
        (row["hasPassword"].clone(), row["hasCode"].clone()),
        (json!(true), json!(true))
    );
    let detail = r.ok(at("item")).await;
    let shown = detail.to_string();
    assert!(
        !shown.contains("pw-ONE-xyz")
            && !shown.contains("pin-7777")
            && !shown.contains("JBSWY3DPEHPK3PXP")
    );
    assert_eq!(
        detail["hidden"],
        json!(["/login/password", "/login/totp", "/custom_fields/0/value"])
    );
    assert_eq!(detail["code"]["code"].as_str().unwrap().len(), 6);
    assert_eq!(detail["readOnly"], false);

    // Show: one hidden value, and only a hidden one.
    let mut reveal = at("reveal");
    reveal["path"] = json!("/login/password");
    assert_eq!(r.ok(reveal.clone()).await, json!("pw-ONE-xyz"));
    reveal["path"] = json!("/login/username");
    assert_eq!(r.ask(reveal).await, Err("invalid_request".to_owned()));

    // Edit: the whole item, without history or passkeys; the old password
    // goes to the history.
    let mut edited = r.ok(at("itemForEdit")).await;
    assert_eq!(edited["login"]["password"], "pw-ONE-xyz");
    assert!(edited.get("history").is_none() && edited["login"].get("passkeys").is_none());
    edited["login"]["password"] = json!("pw-TWO-xyz");
    r.ok(json!({"kind": "saveItem", "vaultId": vault_id, "item": edited}))
        .await;
    let detail = r.ok(at("item")).await;
    let history = detail["passwordHistory"].as_array().unwrap();
    assert_eq!(history.len(), 1, "{detail}");
    let mut reveal = at("reveal");
    reveal["path"] = history[0]["path"].clone();
    assert_eq!(r.ok(reveal).await, json!("pw-ONE-xyz"));

    // Copy any field by its place.
    let mut copy = at("copy");
    copy["path"] = json!("/custom_fields/0/value");
    r.ok(copy).await;
    assert_eq!(r.board.text().as_deref(), Some("pin-7777"));

    // A card, a folder, and moving the card into it.
    let folder = r.ok(json!({"kind": "newFolder", "title": " Work "})).await;
    let card = r
        .ok(json!({"kind": "saveItem", "vaultId": null, "item": {
            "type": "card", "title": "Visa", "notes": "", "favorite": true,
            "folder_id": folder["itemId"], "tags": [], "custom_fields": [],
            "card": {"cardholder": "A", "number": "4111 1111 1111 1234", "expiry_month": 12,
                     "expiry_year": 2030, "security_code": "999", "brand": "visa"}
        }}))
        .await;
    let vault = r.ok(json!({"kind": "vault"})).await;
    assert_eq!(vault["folders"][0]["title"], "Work", "{vault}");
    let row = rows(&vault)
        .into_iter()
        .find(|i| i["itemId"] == card["itemId"])
        .unwrap();
    assert_eq!(row["subtitle"], "•••• 1234");
    assert_eq!(row["folderId"], folder["itemId"]);
    assert!(!vault.to_string().contains("4111 1111"));
    // A card's month must be 1-12: the core refuses the save.
    assert_eq!(
        r.ask(json!({"kind": "saveItem", "vaultId": null, "item": {
            "type": "card", "title": "Bad", "card": {"expiry_month": 13}
        }}))
        .await,
        Err("invalid_item".to_owned())
    );

    // Trash, restore, delete for good.
    r.ok(at("trashItem")).await;
    let trashed = |v: &Value| {
        rows(v)
            .into_iter()
            .find(|i| i["itemId"] == item_id)
            .map(|i| i["trashed"].clone())
    };
    assert_eq!(
        trashed(&r.ok(json!({"kind": "vault"})).await),
        Some(json!(true))
    );
    r.ok(at("restoreItem")).await;
    assert_eq!(
        trashed(&r.ok(json!({"kind": "vault"})).await),
        Some(json!(false))
    );
    r.ok(at("deleteItem")).await;
    assert_eq!(trashed(&r.ok(json!({"kind": "vault"})).await), None);
    // Everything reaches the server.
    let status = r.ok(json!({"kind": "sync"})).await;
    assert_eq!(status["syncError"], Value::Null, "{status}");
}

/// Windows Hello unlock (D68) under D45's rules, with the fake key.
async fn windows_hello(r: &mut Rig) {
    const DAY: u64 = 24 * 60 * 60 * 1000;
    let state = |r: &Rig| r.hello.0.state.lock().unwrap().prompts;
    assert_eq!(r.ok(json!({"kind": "touchId"})).await, json!(false));
    assert_eq!(r.ok(json!({"kind": "biometricOffer"})).await, json!(true));

    // Turning it on: the key, then the challenge signed twice.
    let before = state(r);
    assert_eq!(
        r.ok(json!({"kind": "setTouchId", "on": true})).await,
        json!(true)
    );
    assert_eq!(state(r) - before, 3);
    assert_eq!(r.ok(json!({"kind": "touchId"})).await, json!(true));
    assert_eq!(r.ok(json!({"kind": "biometricOffer"})).await, json!(false));
    for key in [
        name::HELLO_BLOB,
        name::HELLO_SEALED_KEY,
        name::HELLO_CHALLENGE,
    ] {
        assert!(r.keys.get(key).await.unwrap().is_some(), "{key}");
    }

    // Locked, then unlocked with Windows Hello; the blob is made again.
    r.ok(json!({"kind": "lock"})).await;
    let status = r.ok(json!({"kind": "status"})).await;
    assert_eq!(
        (status["state"].clone(), status["touchId"].clone()),
        (json!("locked"), json!(true))
    );
    let blob = r.keys.get(name::HELLO_BLOB).await.unwrap().unwrap();
    let (reply, after) = r
        .backend
        .handle(request(json!({"kind": "unlockTouchId"})))
        .await;
    assert!(reply.is_ok(), "{reply:?}");
    assert_eq!(after, After::Sync);
    assert_eq!(r.ok(json!({"kind": "status"})).await["state"], "unlocked");
    let fresh = r.keys.get(name::HELLO_BLOB).await.unwrap().unwrap();
    assert_ne!(fresh.expose_secret(), blob.expose_secret());

    // A cancel leaves it locked, and on.
    r.ok(json!({"kind": "lock"})).await;
    r.hello.0.state.lock().unwrap().cancel_next = true;
    assert_eq!(
        r.ask(json!({"kind": "unlockTouchId"})).await,
        Err("biometric_cancelled".to_owned())
    );
    assert_eq!(r.ok(json!({"kind": "status"})).await["state"], "locked");

    // Every 14 days the master password, which starts the 14 days again.
    r.clocks.advance(14 * DAY);
    assert_eq!(
        r.ask(json!({"kind": "unlockTouchId"})).await,
        Err("master_password_due".to_owned())
    );
    unlock(r).await;
    r.ok(json!({"kind": "lock"})).await;
    r.ok(json!({"kind": "unlockTouchId"})).await;

    // A key that changed (a new PIN, a reset): off, and the password next.
    r.ok(json!({"kind": "lock"})).await;
    for material in r.hello.0.state.lock().unwrap().keys.values_mut() {
        *material = material.wrapping_add(7);
    }
    assert_eq!(
        r.ask(json!({"kind": "unlockTouchId"})).await,
        Err("hello_changed".to_owned())
    );
    assert_eq!(r.ok(json!({"kind": "touchId"})).await, json!(false));
    unlock(r).await;

    // A computer whose signatures differ, or without Windows Hello: it says so.
    r.hello.0.state.lock().unwrap().nondeterministic = true;
    assert_eq!(
        r.ask(json!({"kind": "setTouchId", "on": true})).await,
        Err("hello_unavailable".to_owned())
    );
    r.hello.0.state.lock().unwrap().nondeterministic = false;
    r.hello.0.state.lock().unwrap().unsupported = true;
    assert_eq!(
        r.ask(json!({"kind": "setTouchId", "on": true})).await,
        Err("hello_unavailable".to_owned())
    );
    r.hello.0.state.lock().unwrap().unsupported = false;

    // Off deletes everything kept for it.
    r.ok(json!({"kind": "setTouchId", "on": true})).await;
    assert_eq!(
        r.ok(json!({"kind": "setTouchId", "on": false})).await,
        json!(false)
    );
    for key in [
        name::HELLO_BLOB,
        name::HELLO_SEALED_KEY,
        name::HELLO_CHALLENGE,
    ] {
        assert!(r.keys.get(key).await.unwrap().is_none(), "{key}");
    }
    assert!(r.hello.0.state.lock().unwrap().keys.is_empty());
}

/// The registry under `HKEY_CURRENT_USER`, in memory.
#[derive(Clone, Default)]
struct Registry(Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>);

impl pwm_desktop::browser_hosts::Registry for Registry {
    fn get(&self, path: &str) -> Option<String> {
        self.0.lock().unwrap().get(path).cloned()
    }
    fn set(
        &self,
        path: &str,
        value: &str,
    ) -> Result<(), pwm_desktop::browser_hosts::RegistryError> {
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

/// The browser helper's enrolments, in memory.
#[derive(Clone, Default)]
struct Enrolments(Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>>);

impl pwm_browser_helper::Store for Enrolments {
    fn get(&self, id: &str) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(id).cloned()
    }
    fn set(&self, id: &str, record: &[u8]) -> Result<(), pwm_browser_helper::StoreError> {
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

#[tokio::test]
async fn the_browser_unlock_switch_registers_the_helper() {
    // Linux: no helper, so it stays off.
    let mut r = rig(true);
    assert_eq!(r.ok(json!({"kind": "browserUnlock"})).await, json!(false));
    assert_eq!(
        r.ask(json!({"kind": "setBrowserUnlock", "on": true}))
            .await
            .unwrap_err(),
        "unsupported"
    );

    // Windows: the helper next to the app, keys in the registry.
    let dir = std::env::temp_dir().join(format!("pwm-backend-hosts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm-browser-helper.exe"), b"").unwrap();
    let (registry, enrolments) = (Registry::default(), Enrolments::default());
    let Rig {
        backend,
        keys,
        board,
        clocks,
        hello,
    } = r;
    let mut r = Rig {
        backend: backend.with_browser_hosts(pwm_desktop::browser_hosts::BrowserHosts::new(
            dir.join("NativeMessagingHosts"),
            dir.join("pwm-browser-helper.exe"),
            Box::new(registry.clone()),
            Box::new(enrolments.clone()),
        )),
        keys,
        board,
        clocks,
        hello,
    };

    // Not without Windows Hello set up.
    r.hello.0.state.lock().unwrap().unsupported = true;
    assert_eq!(
        r.ask(json!({"kind": "setBrowserUnlock", "on": true}))
            .await
            .unwrap_err(),
        "hello_unavailable"
    );
    r.hello.0.state.lock().unwrap().unsupported = false;

    assert_eq!(
        r.ok(json!({"kind": "setBrowserUnlock", "on": true})).await,
        json!(true)
    );
    assert_eq!(r.ok(json!({"kind": "browserUnlock"})).await, json!(true));
    assert_eq!(registry.0.lock().unwrap().len(), 3);

    // Off forgets what the helper kept, with its Windows Hello key.
    let id = "0123456789abcdef0123456789abcdef";
    pwm_browser_helper::Store::set(&enrolments, id, b"record").unwrap();
    r.hello
        .create(&pwm_browser_helper::credential_name(id))
        .unwrap();
    assert_eq!(
        r.ok(json!({"kind": "setBrowserUnlock", "on": false})).await,
        json!(false)
    );
    assert!(registry.0.lock().unwrap().is_empty());
    assert!(enrolments.0.lock().unwrap().is_empty());
    assert!(r.hello.0.state.lock().unwrap().keys.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
