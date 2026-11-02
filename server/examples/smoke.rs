//! Smoke test for a running server (D30): registers an account, logs in on a
//! second device, reads the account and the vault list. CI runs it against
//! the compose stack:
//!
//! ```text
//! cargo run -p pwm-server --example smoke -- http://127.0.0.1:8080
//! ```
//!
//! Plain HTTP/1.1 over a socket, so it needs no HTTP client crate.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::{
    AccountKeyset, KdfParams, OpaqueLogin, OpaqueRegistration, Secret, SymmetricKey, generate_id,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(value: &Value) -> Result<Vec<u8>, String> {
    let text = value.as_str().ok_or("expected a string")?;
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| "not base64url".to_owned())
}

fn new_id() -> Result<Uuid, String> {
    Ok(Uuid::from_bytes(generate_id().map_err(|e| e.to_string())?))
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Value,
}

/// One request on a new connection (`Connection: close`).
fn call(
    host: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Result<Reply, String> {
    let mut stream = TcpStream::connect(host).map_err(|e| format!("connect {host}: {e}"))?;
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{auth}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8(raw).map_err(|_| "response is not UTF-8")?;
    let (head, body) = text.split_once("\r\n\r\n").ok_or("malformed response")?;
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("no status line")?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    // `/healthz` answers plain text; the API answers JSON.
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.to_owned()))
    };
    Ok(Reply {
        status,
        headers,
        body,
    })
}

fn expect(reply: &Reply, status: u16, what: &str) -> Result<(), String> {
    if reply.status == status {
        Ok(())
    } else {
        Err(format!(
            "{what}: expected {status}, got {} {}",
            reply.status, reply.body
        ))
    }
}

fn run(base: &str) -> Result<(), String> {
    let host = base
        .strip_prefix("http://")
        .ok_or("give an http:// base URL")?
        .trim_end_matches('/');

    let health = call(host, "GET", "/healthz", None, None)?;
    expect(&health, 200, "healthz")?;

    // Register: the same steps an app takes (D26).
    let account = new_id()?;
    let auth_secret = Secret::new([7u8; 32]);
    let keyset = AccountKeyset::generate().map_err(|e| e.to_string())?;
    let auk = SymmetricKey::generate().map_err(|e| e.to_string())?;
    let sealed = keyset
        .seal(&auk, account.as_bytes())
        .map_err(|e| e.to_string())?;
    let (client, request) = OpaqueRegistration::start(&auth_secret).map_err(|e| e.to_string())?;
    let reply = call(
        host,
        "POST",
        "/v1/auth/register/start",
        None,
        Some(json!({ "account_id": account, "registration_request": b64(&request) })),
    )?;
    expect(&reply, 200, "register/start")?;
    let upload = client
        .finish(
            &auth_secret,
            account.as_bytes(),
            &unb64(&reply.body["registration_response"])?,
        )
        .map_err(|e| e.to_string())?;
    let laptop = new_id()?;
    let name =
        |device: &Uuid| keyset.seal_device_name(account.as_bytes(), device.as_bytes(), "smoke");
    let laptop_name = name(&laptop).map_err(|e| e.to_string())?;
    let d = KdfParams::DEFAULT;
    let reply = call(
        host,
        "POST",
        "/v1/auth/register/finish",
        None,
        Some(json!({
            "email": format!("smoke-{account}@example.com"), "account_id": account,
            "registration_upload": b64(&upload), "salt": b64(&[1u8; 16]),
            "kdf": { "memory_kib": d.memory_kib, "iterations": d.iterations, "parallelism": d.parallelism },
            "sealed_keyset": b64(&sealed.to_bytes()), "x25519_pub": b64(&keyset.x25519_public()),
            "ed25519_pub": b64(&keyset.ed25519_public()),
            "device": { "id": laptop, "platform": "linux", "name_sealed": b64(&laptop_name.to_bytes()) },
        })),
    )?;
    expect(&reply, 201, "register/finish")?;

    // Log in on a second device.
    let (login, request) = OpaqueLogin::start(&auth_secret).map_err(|e| e.to_string())?;
    let reply = call(
        host,
        "POST",
        "/v1/auth/login/start",
        None,
        Some(json!({ "account_id": account, "credential_request": b64(&request) })),
    )?;
    expect(&reply, 200, "login/start")?;
    let finalization = login
        .finish(
            &auth_secret,
            account.as_bytes(),
            &unb64(&reply.body["credential_response"])?,
        )
        .map_err(|e| e.to_string())?;
    let phone = new_id()?;
    let phone_name = name(&phone).map_err(|e| e.to_string())?;
    let reply = call(
        host,
        "POST",
        "/v1/auth/login/finish",
        None,
        Some(json!({
            "login_id": reply.body["login_id"], "credential_finalization": b64(&finalization),
            "device": { "id": phone, "platform": "android", "name_sealed": b64(&phone_name.to_bytes()) },
        })),
    )?;
    expect(&reply, 200, "login/finish")?;
    let token = reply.body["access_token"]
        .as_str()
        .ok_or("no access token")?
        .to_owned();

    let reply = call(host, "GET", "/v1/account", Some(&token), None)?;
    expect(&reply, 200, "account")?;
    if unb64(&reply.body["sealed_keyset"])? != sealed.to_bytes() {
        return Err("the keyset did not round-trip".to_owned());
    }
    let reply = call(host, "GET", "/v1/vaults", Some(&token), None)?;
    expect(&reply, 200, "vaults")?;
    if !reply.headers.iter().any(|(n, _)| n == "pwm-epoch") {
        return Err("no pwm-epoch header".to_owned());
    }
    Ok(())
}

fn main() -> ExitCode {
    let base = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_owned());
    match run(&base) {
        Ok(()) => {
            println!("smoke test passed against {base}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("smoke test failed: {err}");
            ExitCode::FAILURE
        }
    }
}
