//! Writes `core/test-vectors/passkey.json` (D37),
//! `passkey_hashed.json` (the calls a platform makes with a client data
//! hash, D47) and `passkey_android.json` (Android's Credential Manager, an
//! app and a privileged browser, D61) from fixed inputs. ES256 signatures here are deterministic
//! (RFC 6979), so every output is exact. Existing files are never
//! overwritten: published vectors freeze v1.
//!
//!     cargo run -p pwm-passkey --example gen_vectors

use std::fs;
use std::path::Path;

use pwm_crypto::Secret;
use pwm_passkey::{
    AndroidCaller, Caller, CreateRequest, GetRequest, authenticate, authenticate_android,
    authenticate_hashed, register, register_android, register_hashed,
};
use serde_json::json;
use sha2::Digest;

fn main() {
    browser();
    hashed();
    android();
}

fn android() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-vectors/passkey_android.json");
    if path.exists() {
        println!("{} exists; not overwritten", path.display());
        return;
    }
    let seed = [0x42u8; 32];
    let credential_id = [0xC7u8; 16];
    let cert = [0xABu8; 32];
    let create = json!({
        "challenge": "cmVnaXN0ZXItY2hhbGxlbmdlLTAwMDE",
        "rp": {"id": "example.com", "name": "Example"},
        "user": {"id": "dXNlci1oYW5kbGUtMQ", "name": "ada@example.com", "displayName": "Ada"},
        "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
    })
    .to_string();
    let get = json!({
        "challenge": "c2lnbi1pbi1jaGFsbGVuZ2UtMDAwMQ",
        "rpId": "example.com",
    })
    .to_string();
    // What Chrome would hash: any bytes do, the provider never sees them.
    let hash: [u8; 32] = sha2::Sha256::digest(b"browser-client-data-0001").into();
    let callers = [
        (
            "app",
            AndroidCaller::App {
                package: "com.example.app",
                cert_sha256: &cert,
            },
        ),
        (
            "browser",
            AndroidCaller::Browser {
                origin: "https://login.example.com",
                client_data_hash: &hash,
            },
        ),
    ];
    let vectors: Vec<serde_json::Value> = callers
        .iter()
        .map(|(name, caller)| {
            let registration = register_android(
                *caller,
                &CreateRequest::from_json(&create).unwrap(),
                &Secret::new(seed),
                credential_id,
            )
            .unwrap();
            let assertion = authenticate_android(
                *caller,
                &GetRequest::from_json(&get).unwrap(),
                &registration.passkey,
            )
            .unwrap();
            let mut v = json!({
                "name": name,
                "key_seed": hex::encode(seed),
                "credential_id": hex::encode(credential_id),
                "create_request": create,
                "registration": serde_json::from_str::<serde_json::Value>(&registration.response_json).unwrap(),
                "private_key_pkcs8": hex::encode(registration.passkey.private_key.expose_secret()),
                "get_request": get,
                "assertion": serde_json::from_str::<serde_json::Value>(&assertion).unwrap(),
            });
            match caller {
                AndroidCaller::App {
                    package,
                    cert_sha256,
                } => {
                    v["package"] = json!(package);
                    v["cert_sha256"] = json!(hex::encode(cert_sha256));
                }
                AndroidCaller::Browser {
                    origin,
                    client_data_hash,
                } => {
                    v["origin"] = json!(origin);
                    v["client_data_hash"] = json!(hex::encode(client_data_hash));
                }
            }
            v
        })
        .collect();
    let doc = json!({
        "description": "Passkeys through Android's Credential Manager v1 (D61): an app's client data with the android:apk-key-hash origin and its package, and a privileged browser's client data hash with a placeholder clientDataJSON. ES256, fmt none, sign count 0",
        "format_version": 1,
        "operation": "passkey_android",
        "vectors": vectors,
    });
    fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    println!("wrote {}", path.display());
}

fn hashed() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-vectors/passkey_hashed.json");
    if path.exists() {
        println!("{} exists; not overwritten", path.display());
        return;
    }
    let seed = [0x42u8; 32];
    let credential_id = [0xC7u8; 16];
    let user_handle = b"user-handle-1";
    let registration = register_hashed(
        "example.com",
        user_handle,
        "ada@example.com",
        &Secret::new(seed),
        credential_id,
    )
    .unwrap();
    // What iOS would hash: any bytes do, the authenticator never sees them.
    let client_data_hash: [u8; 32] = sha2::Sha256::digest(b"client-data-0001").into();
    let assertion =
        authenticate_hashed("example.com", &client_data_hash, &registration.passkey).unwrap();
    let doc = json!({
        "description": "Passkey registration and assertion from a client data hash v1 (D47): ES256, fmt none, sign count 0",
        "format_version": 1,
        "operation": "passkey_hashed",
        "vectors": [{
            "rp_id": "example.com",
            "user_handle": hex::encode(user_handle),
            "user_name": "ada@example.com",
            "key_seed": hex::encode(seed),
            "credential_id": hex::encode(credential_id),
            "attestation_object": hex::encode(&registration.attestation_object),
            "private_key_pkcs8": hex::encode(registration.passkey.private_key.expose_secret()),
            "client_data_hash": hex::encode(client_data_hash),
            "authenticator_data": hex::encode(&assertion.authenticator_data),
            "signature": hex::encode(&assertion.signature),
        }],
    });
    fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    println!("wrote {}", path.display());
}

fn browser() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-vectors/passkey.json");
    if path.exists() {
        println!("{} exists; not overwritten", path.display());
        return;
    }
    let origin = "https://login.example.com";
    let caller = Caller {
        origin,
        top_origin: None,
    };
    let seed = [0x42u8; 32];
    let credential_id = [0xC7u8; 16];
    let create = json!({
        "challenge": "cmVnaXN0ZXItY2hhbGxlbmdlLTAwMDE",
        "rp": {"id": "example.com", "name": "Example"},
        "user": {"id": "dXNlci1oYW5kbGUtMQ", "name": "ada@example.com", "displayName": "Ada"},
        "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
    })
    .to_string();
    let get = json!({
        "challenge": "c2lnbi1pbi1jaGFsbGVuZ2UtMDAwMQ",
        "rpId": "example.com",
        "allowCredentials": [{"type": "public-key", "id": "x8fHx8fHx8fHx8fHx8fHxw"}],
    })
    .to_string();
    let registration = register(
        caller,
        &CreateRequest::from_json(&create).unwrap(),
        &Secret::new(seed),
        credential_id,
    )
    .unwrap();
    let assertion = authenticate(
        caller,
        &GetRequest::from_json(&get).unwrap(),
        &registration.passkey,
    )
    .unwrap();
    let doc = json!({
        "description": "Passkey registration and assertion v1 (D37): ES256, fmt none, sign count 0",
        "format_version": 1,
        "operation": "passkey",
        "vectors": [{
            "origin": origin,
            "key_seed": hex::encode(seed),
            "credential_id": hex::encode(credential_id),
            "create_request": create,
            "registration": serde_json::from_str::<serde_json::Value>(&registration.response_json).unwrap(),
            "private_key_pkcs8": hex::encode(registration.passkey.private_key.expose_secret()),
            "get_request": get,
            "assertion": serde_json::from_str::<serde_json::Value>(&assertion).unwrap(),
        }],
    });
    fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    println!("wrote {}", path.display());
}
