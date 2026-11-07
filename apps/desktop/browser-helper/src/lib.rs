//! The browser helper on Windows (D69): the browser starts it for the
//! extension's "Unlock with Windows Hello", with D48's protocol unchanged.
//! It keeps the extension's key K sealed under a key derived from a Windows
//! Hello signature (the core's `seal_browser_hello`), one enrolment per
//! calling extension, account and device. K never touches the disk.
//!
//! The protocol: one framed JSON message in, one out. `enroll {account_id,
//! device_id, key}`, `unlock {account_id, device_id}` and `forget
//! {account_id, device_id}`, with `"v": 1`. Errors: `refused`, `invalid`,
//! `unknown`, `cancelled`, `changed`, `unavailable`.
#![forbid(unsafe_code)]

use std::io::{self, Read, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use pwm_crypto::{
    Envelope, Secret, hkdf_sha256, open_browser_hello, seal_browser_hello, windows_hello_challenge,
};
use pwm_hello::{HelloError, HelloKey};
use serde::{Deserialize, Serialize};

/// The protocol's version (`BrowserUnlock.version` in PWMKit).
pub const VERSION: u8 = 1;
/// The native-messaging host's name, the extension's `NATIVE_HOST`.
pub const HOST_NAME: &str = "invalid.pwm.browser";
/// Our extension in Chromium browsers and Firefox: placeholders until the
/// stores give the real ids (M10), the same as the Mac helper's.
pub const CHROMIUM_ORIGINS: &[&str] = &["chrome-extension://pppppppppppppppppppppppppppppppp/"];
pub const FIREFOX_IDS: &[&str] = &["pwm@pwm.invalid"];
/// Requests are a few hundred bytes; nothing larger is read.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// The format of what's kept per enrolment: this byte, the challenge, then
/// the envelope.
const RECORD_VERSION: u8 = 1;

/// The caller from the helper's arguments: Chromium browsers pass the
/// extension's origin first; Firefox passes the manifest's path, then the
/// extension's id.
pub fn caller(args: &[String]) -> Option<String> {
    let args = args.get(1..)?;
    match args.first() {
        Some(first) if first.starts_with("chrome-extension://") => Some(first.clone()),
        _ => args.get(1).cloned(),
    }
}

/// Where the browsers find the helper on Windows (D69 §4): each one's key
/// under `HKEY_CURRENT_USER`, and whether it reads Firefox's manifest form.
/// Brave and Opera read Chrome's key.
pub const REGISTRY_KEYS: &[(&str, bool)] = &[
    (
        r"Software\Google\Chrome\NativeMessagingHosts\invalid.pwm.browser",
        false,
    ),
    (
        r"Software\Microsoft\Edge\NativeMessagingHosts\invalid.pwm.browser",
        false,
    ),
    (
        r"Software\Mozilla\NativeMessagingHosts\invalid.pwm.browser",
        true,
    ),
];

/// A native-messaging manifest naming the helper at `helper_path`, for
/// Chromium browsers (`allowed_origins`) or Firefox (`allowed_extensions`).
pub fn manifest(firefox: bool, helper_path: &str) -> String {
    let mut json = serde_json::json!({
        "name": HOST_NAME,
        "description": "PWM: unlock with Windows Hello",
        "path": helper_path,
        "type": "stdio",
    });
    if firefox {
        json["allowed_extensions"] = serde_json::json!(FIREFOX_IDS);
    } else {
        json["allowed_origins"] = serde_json::json!(CHROMIUM_ORIGINS);
    }
    serde_json::to_string_pretty(&json).unwrap_or_default()
}

/// Every caller this build accepts.
pub fn callers() -> Vec<String> {
    CHROMIUM_ORIGINS
        .iter()
        .chain(FIREFOX_IDS)
        .map(|c| (*c).to_owned())
        .collect()
}

/// Reads one message: a 4-byte length in the machine's byte order, then
/// JSON. None at the end of input.
pub fn read_message(mut input: impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match input.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
    }
    let mut message = vec![0; len];
    input.read_exact(&mut message)?;
    Ok(Some(message))
}

pub fn write_message(mut output: impl Write, message: &[u8]) -> io::Result<()> {
    let len = u32::try_from(message.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "too large"))?;
    output.write_all(&len.to_ne_bytes())?;
    output.write_all(message)?;
    output.flush()
}

/// The record couldn't be kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreError;

/// Where each enrolment's record is kept: DPAPI files on Windows, memory in
/// tests.
pub trait Store {
    fn get(&self, id: &str) -> Option<Vec<u8>>;
    fn set(&self, id: &str, record: &[u8]) -> Result<(), StoreError>;
    fn delete(&self, id: &str);
    /// Every enrolment kept, for turning browser unlock off.
    fn ids(&self) -> Vec<String>;
}

#[cfg(windows)]
pub mod store;

#[derive(Deserialize)]
struct Request {
    v: u8,
    #[serde(rename = "type")]
    kind: String,
    account_id: String,
    device_id: String,
    key: Option<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct Reply {
    pub v: u8,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}

impl Reply {
    fn done() -> Self {
        Self {
            v: VERSION,
            ok: true,
            key: None,
            error: None,
        }
    }

    pub fn failed(error: &'static str) -> Self {
        Self {
            v: VERSION,
            ok: false,
            key: None,
            error: Some(error),
        }
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self)
            .unwrap_or_else(|_| br#"{"v":1,"ok":false,"error":"invalid"}"#.to_vec())
    }
}

fn hello_error(error: HelloError) -> &'static str {
    match error {
        HelloError::Cancelled => "cancelled",
        HelloError::Changed => "changed",
        HelloError::Unavailable => "unavailable",
    }
}

/// An id: 32 lowercase hex digits, as the extension sends them.
fn parse_id(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut id = [0u8; 16];
    for (i, pair) in text.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(pair).ok()?;
        id[i] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(id)
}

/// The enrolment's name, for the key credential and the file: a digest of
/// the caller, account and device, so neither shows on disk.
pub fn enrolment(caller: &str, account_id: &str, device_id: &str) -> String {
    let joined = format!("{caller}\n{account_id}\n{device_id}");
    let digest = hkdf_sha256(joined.as_bytes(), b"", b"pwm/browser-helper/enrolment/v1");
    digest.expose_secret()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The Windows Hello key credential's name for an enrolment.
pub fn credential_name(id: &str) -> String {
    format!("PWM browser {id}")
}

pub struct Handler<'a> {
    callers: Vec<String>,
    hello: &'a dyn HelloKey,
    store: &'a dyn Store,
}

impl<'a> Handler<'a> {
    pub fn new(callers: Vec<String>, hello: &'a dyn HelloKey, store: &'a dyn Store) -> Self {
        Self {
            callers,
            hello,
            store,
        }
    }

    /// One request's reply, as JSON.
    pub fn handle(&self, message: &[u8], caller: Option<&str>) -> Vec<u8> {
        self.answer(message, caller).to_json()
    }

    pub fn answer(&self, message: &[u8], caller: Option<&str>) -> Reply {
        let Some(caller) = caller.filter(|c| self.callers.iter().any(|known| known == c)) else {
            return Reply::failed("refused");
        };
        let Ok(request) = serde_json::from_slice::<Request>(message) else {
            return Reply::failed("invalid");
        };
        let (Some(account), Some(device)) =
            (parse_id(&request.account_id), parse_id(&request.device_id))
        else {
            return Reply::failed("invalid");
        };
        if request.v != VERSION {
            return Reply::failed("invalid");
        }
        let id = enrolment(caller, &request.account_id, &request.device_id);
        let name = credential_name(&id);
        match request.kind.as_str() {
            "enroll" => {
                let key = request
                    .key
                    .as_deref()
                    .and_then(|text| STANDARD.decode(text).ok())
                    .and_then(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok())
                    .map(Secret::new);
                match key {
                    Some(key) => self.enroll(&id, &name, &key, caller, &account, &device),
                    None => Reply::failed("invalid"),
                }
            }
            "unlock" => self.unlock(&id, &name, caller, &account, &device),
            "forget" => {
                self.forget(&id, &name);
                Reply::done()
            }
            _ => Reply::failed("invalid"),
        }
    }

    fn forget(&self, id: &str, name: &str) {
        self.hello.delete(name);
        self.store.delete(id);
    }

    /// Windows Hello makes a key and signs the challenge twice: the two
    /// signatures must match (D68 §2), or unlocking would fail later.
    fn enroll(
        &self,
        id: &str,
        name: &str,
        key: &Secret<[u8; 32]>,
        caller: &str,
        account: &[u8; 16],
        device: &[u8; 16],
    ) -> Reply {
        if !self.hello.supported() {
            return Reply::failed("unavailable");
        }
        if let Err(error) = self.hello.create(name) {
            return Reply::failed(hello_error(error));
        }
        let sealed = (|| {
            let challenge = windows_hello_challenge().map_err(|_| "unavailable")?;
            let first = self
                .hello
                .sign(name, challenge.expose_secret())
                .map_err(hello_error)?;
            let second = self
                .hello
                .sign(name, challenge.expose_secret())
                .map_err(hello_error)?;
            if first.expose_secret() != second.expose_secret() {
                return Err("unavailable");
            }
            let envelope = seal_browser_hello(
                key,
                first.expose_secret(),
                &challenge,
                caller,
                account,
                device,
            )
            .map_err(|_| "unavailable")?;
            let mut record = vec![RECORD_VERSION];
            record.extend_from_slice(challenge.expose_secret());
            record.extend_from_slice(&envelope.to_bytes());
            self.store
                .set(id, &record)
                .map_err(|StoreError| "unavailable")
        })();
        match sealed {
            Ok(()) => Reply::done(),
            Err(error) => {
                self.forget(id, name);
                Reply::failed(error)
            }
        }
    }

    fn unlock(
        &self,
        id: &str,
        name: &str,
        caller: &str,
        account: &[u8; 16],
        device: &[u8; 16],
    ) -> Reply {
        let Some(record) = self.store.get(id) else {
            return Reply::failed("unknown");
        };
        let parsed = (record.len() > 33 && record[0] == RECORD_VERSION)
            .then(|| {
                let challenge = Secret::<[u8; 32]>::new_with(|c| c.copy_from_slice(&record[1..33]));
                Envelope::from_bytes(&record[33..])
                    .ok()
                    .map(|envelope| (challenge, envelope))
            })
            .flatten();
        let Some((challenge, envelope)) = parsed else {
            self.forget(id, name);
            return Reply::failed("changed");
        };
        let signature = match self.hello.sign(name, challenge.expose_secret()) {
            Ok(signature) => signature,
            Err(HelloError::Changed) => {
                self.forget(id, name);
                return Reply::failed("changed");
            }
            Err(error) => return Reply::failed(hello_error(error)),
        };
        // A signature that doesn't open K: the key changed (a reset).
        match open_browser_hello(
            &envelope,
            signature.expose_secret(),
            &challenge,
            caller,
            account,
            device,
        ) {
            Ok(key) => Reply {
                v: VERSION,
                ok: true,
                key: Some(STANDARD.encode(key.expose_secret())),
                error: None,
            },
            Err(_) => {
                self.forget(id, name);
                Reply::failed("changed")
            }
        }
    }
}

#[cfg(test)]
mod tests;
