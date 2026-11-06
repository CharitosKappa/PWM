//! Writes the JSON test vectors in `core/test-vectors/` using only the public
//! API. Existing files are never overwritten: published vectors freeze the v1
//! formats. To add vectors for a new operation, add a generator here and run:
//!
//!     cargo run -p pwm-crypto --example gen_vectors
//!
//! Inputs are fixed; nonces and ephemeral keys are random and recorded, so
//! the checkers (`tests/vectors.rs`, `core/test-vectors/verify.mjs`) verify
//! the decrypt direction plus every deterministic output.

use std::fs;
use std::path::{Path, PathBuf};

use pwm_crypto::{
    Aad, AccountKeyset, ApprovalKeys, Id, KdfParams, OpaqueLogin, OpaqueRegistration, Secret,
    SecretKey, ServerSecrets, SignatureContext, SymmetricKey, approval_commitment, create_account,
    derive_account_keys, derive_password_key, hkdf_sha256, normalize_password, opaque_record, open,
    open_export, seal,
};
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use serde_json::{Value, json};

const ACCOUNT_A: Id = [0xA1; 16];
const ACCOUNT_B: Id = [0xA2; 16];
const VAULT: Id = [0xB1; 16];
const ITEM: Id = [0xC1; 16];
const DEVICE: Id = [0xD1; 16];

/// File name, description, generator.
type Generator = (&'static str, &'static str, fn() -> Vec<Value>);

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test-vectors");
    let files: [Generator; 18] = [
        ("aad", "AAD byte layouts (D11, D17)", aad),
        ("aead", "XChaCha20-Poly1305 v1 envelopes (D10)", aead),
        ("hkdf", "HKDF-SHA256, 32-byte output (D13)", hkdf),
        ("kdf", "Argon2id password key (D12, D15)", kdf),
        ("password", "Master password normalization (D16)", password),
        ("secret_key", "Secret Key text format (D14)", secret_key),
        (
            "account",
            "AUK, auth secret and sealed keyset (D13, keyset v1)",
            account,
        ),
        (
            "vault_key",
            "Vault key wrapped under sym_key (vault key v1)",
            vault_key,
        ),
        (
            "signature",
            "Domain-separated Ed25519 signatures (D17)",
            signature,
        ),
        ("share", "X25519 vault shares v1 (D17)", share),
        (
            "export",
            "Password-protected .pwmx export files v1 (D24)",
            export,
        ),
        (
            "device_name",
            "Device names sealed under sym_key v1 (D26)",
            device_name,
        ),
        (
            "opaque",
            "OPAQUE registration and login transcript with seeded randomness (D26)",
            opaque,
        ),
        (
            "session",
            "Extension session blob and sealed device secret v1 (D34)",
            session,
        ),
        (
            "biometric_unlock",
            "Face ID / Touch ID unlock blob v1 (D45)",
            biometric_unlock,
        ),
        (
            "approval",
            "Approving a new device: commitment, code and sealed Secret Key v1 (D46)",
            approval,
        ),
        (
            "browser_unlock",
            "Touch ID unlock for the browser extension through the Mac app v1 (D48)",
            browser_unlock,
        ),
        (
            "windows_hello",
            "The biometric unlock key under a Windows Hello signature's key v1 (D68)",
            windows_hello,
        ),
    ];
    for (name, description, generate) in files {
        write(&dir, name, description, generate());
    }
}

fn write(dir: &Path, name: &str, description: &str, vectors: Vec<Value>) {
    let path = dir.join(format!("{name}.json"));
    if path.exists() {
        println!("kept    {} (vectors are frozen)", path.display());
        return;
    }
    let doc = json!({
        "format_version": 1,
        "operation": name,
        "description": description,
        "vectors": vectors,
    });
    fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
    println!("wrote   {}", path.display());
}

fn h(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

fn key(fill: u8) -> SymmetricKey {
    SymmetricKey::from_secret(Secret::new([fill; 32]))
}

fn params_json(p: &KdfParams) -> Value {
    json!({ "memory_kib": p.memory_kib, "iterations": p.iterations, "parallelism": p.parallelism })
}

/// A keyset with known secrets, built through the public seal/open API.
fn known_keyset(
    auk: &SymmetricKey,
    account: &Id,
    sym: u8,
    x: u8,
    ed: u8,
) -> (AccountKeyset, Vec<u8>, [u8; 97]) {
    let mut plaintext = [0u8; 97];
    plaintext[0] = 1;
    plaintext[1..33].fill(sym);
    plaintext[33..65].fill(x);
    plaintext[65..97].fill(ed);
    let sealed = seal(auk, &plaintext, &Aad::keyset(1, account)).unwrap();
    let keyset = AccountKeyset::open(&sealed, auk, account).unwrap();
    (keyset, sealed.to_bytes(), plaintext)
}

fn aad() -> Vec<Value> {
    vec![
        json!({ "context": "item", "version": 1, "vault_id": h(&VAULT), "item_id": h(&ITEM), "revision": "7",
                "aad": h(Aad::item(1, &VAULT, &ITEM, 7).as_bytes()) }),
        json!({ "context": "item", "version": 2, "vault_id": h(&VAULT), "item_id": h(&ITEM), "revision": u64::MAX.to_string(),
                "aad": h(Aad::item(2, &VAULT, &ITEM, u64::MAX).as_bytes()) }),
        json!({ "context": "vault-key", "version": 1, "vault_id": h(&VAULT),
                "aad": h(Aad::vault_key(1, &VAULT).as_bytes()) }),
        json!({ "context": "keyset", "version": 1, "account_id": h(&ACCOUNT_A),
                "aad": h(Aad::keyset(1, &ACCOUNT_A).as_bytes()) }),
        json!({ "context": "share", "version": 1, "vault_id": h(&VAULT), "sender_ed25519_pub": h(&[0xEE; 32]),
                "recipient_x25519_pub": h(&[0xDD; 32]),
                "aad": h(Aad::share(1, &VAULT, &[0xEE; 32], &[0xDD; 32]).as_bytes()) }),
    ]
}

fn aead() -> Vec<Value> {
    let cases: [(&[u8], u8, u64); 3] = [
        (b"", 0x10, 1),
        (b"correct horse battery staple", 0x11, 2),
        (&[0xFF; 300], 0x12, 3),
    ];
    cases
        .iter()
        .map(|&(plaintext, fill, revision)| {
            let aad = Aad::item(1, &VAULT, &ITEM, revision);
            let env = seal(&key(fill), plaintext, &aad).unwrap();
            json!({ "key": h(&[fill; 32]), "aad_context": "item", "version": 1, "vault_id": h(&VAULT),
                    "item_id": h(&ITEM), "revision": revision.to_string(), "aad": h(aad.as_bytes()),
                    "plaintext": h(plaintext), "envelope": h(&env.to_bytes()) })
        })
        .collect()
}

fn hkdf() -> Vec<Value> {
    let cases: [(&[u8], &[u8], &[u8]); 3] = [
        (
            &[0x0b; 22],
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            &[0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9],
        ),
        (&[0x0b; 22], b"", b""),
        (&[0x42; 32], &ACCOUNT_A, b"pwm/auk/v1"),
    ];
    cases
        .iter()
        .map(|&(ikm, salt, info)| {
            json!({ "ikm": h(ikm), "salt": h(salt), "info": h(info),
                    "okm": h(hkdf_sha256(ikm, salt, info).expose_secret()) })
        })
        .collect()
}

fn kdf() -> Vec<Value> {
    let cases: [(&[u8], [u8; 16], KdfParams); 3] = [
        (b"password", [0x01; 16], KdfParams::DEFAULT),
        (b"password", [0x01; 16], KdfParams::MOBILE_FLOOR),
        (
            "caf\u{e9} \u{1F511}".as_bytes(),
            [0x02; 16],
            KdfParams::MOBILE_FLOOR,
        ),
    ];
    cases
        .iter()
        .map(|(pw, salt, params)| {
            let out = derive_password_key(pw, salt, params).unwrap();
            json!({ "password": h(pw), "salt": h(salt), "params": params_json(params),
                    "pw_key": h(out.expose_secret()) })
        })
        .collect()
}

fn password() -> Vec<Value> {
    let inputs = [
        "hunter2",
        "caf\u{e9}",
        "cafe\u{301}",
        "a\u{a0}b\u{3000}c",
        " Trailing ",
        "\u{212b}ngstr\u{f6}m",
        "",
        "tab\there",
        "line\n",
        "nul\u{0}",
    ];
    inputs
        .iter()
        .map(|input| match normalize_password(input) {
            Ok(n) => json!({ "input": h(input.as_bytes()), "normalized": h(n.expose_secret().as_bytes()) }),
            Err(_) => json!({ "input": h(input.as_bytes()), "normalized": null }),
        })
        .collect()
}

fn secret_key() -> Vec<Value> {
    let keys: [[u8; 16]; 4] = [[0; 16], [0xFF; 16], [0x5A; 16], *b"PWM-Secret-Key!!"];
    keys.iter()
        .map(|bytes| {
            let text = SecretKey::from_bytes(Secret::new(*bytes)).to_text();
            json!({ "bytes": h(bytes), "text": text.expose_secret() })
        })
        .collect()
}

fn account() -> Vec<Value> {
    let cases: [(&str, [u8; 16], Id, [u8; 16]); 2] = [
        (
            "correct horse battery staple",
            [0x0F; 16],
            ACCOUNT_A,
            [0x5A; 16],
        ),
        ("caf\u{e9}\u{a0}pw", [0xF0; 16], ACCOUNT_B, [0xA5; 16]),
    ];
    cases
        .iter()
        .map(|(password, sk, account, salt)| {
            let params = KdfParams::MOBILE_FLOOR;
            let normalized = normalize_password(password).unwrap();
            let pw_key = derive_password_key(normalized.expose_secret().as_bytes(), salt, &params).unwrap();
            let sk_key = hkdf_sha256(sk, account, b"pwm/sk/v1");
            let mixed: Vec<u8> = pw_key.expose_secret().iter().zip(sk_key.expose_secret()).map(|(a, b)| a ^ b).collect();
            let auk = hkdf_sha256(&mixed, account, b"pwm/auk/v1");
            let auth = hkdf_sha256(&mixed, account, b"pwm/auth/v1");

            let auk_key = SymmetricKey::from_secret(Secret::new(*auk.expose_secret()));
            let (keyset, sealed, plaintext) = known_keyset(&auk_key, account, 0x21, 0x22, 0x23);
            // Cross-check against the high-level API.
            let secret_key = SecretKey::from_bytes(Secret::new(*sk));
            let keys = derive_account_keys(password, &secret_key, account, salt, &params).unwrap();
            assert_eq!(keys.auth_secret().expose_secret(), auth.expose_secret());
            AccountKeyset::open(&pwm_crypto::Envelope::from_bytes(&sealed).unwrap(), keys.auk(), account).unwrap();

            json!({ "password": h(password.as_bytes()), "secret_key": secret_key.to_text().expose_secret(), "secret_key_bytes": h(sk),
                    "account_id": h(account), "salt": h(salt), "params": params_json(&params),
                    "pw_key": h(pw_key.expose_secret()), "sk_key": h(sk_key.expose_secret()),
                    "auk": h(auk.expose_secret()), "auth_secret": h(auth.expose_secret()),
                    "keyset_plaintext": h(&plaintext), "sealed_keyset": h(&sealed),
                    "x25519_public": h(&keyset.x25519_public()), "ed25519_public": h(&keyset.ed25519_public()) })
        })
        .collect()
}

fn vault_key() -> Vec<Value> {
    let auk = key(0x30);
    let (keyset, sealed, plaintext) = known_keyset(&auk, &ACCOUNT_A, 0x31, 0x32, 0x33);
    let vk = key(0x34);
    let wrapped = keyset.wrap_vault_key(&vk, &VAULT).unwrap();
    let item_aad = Aad::item(1, &VAULT, &ITEM, 1);
    let item = seal(&vk, b"probe item", &item_aad).unwrap();
    assert_eq!(
        open(
            &keyset.unwrap_vault_key(&wrapped, &VAULT).unwrap(),
            &item,
            &item_aad
        )
        .unwrap()
        .expose_secret(),
        b"probe item"
    );
    vec![
        json!({ "auk": h(&[0x30; 32]), "account_id": h(&ACCOUNT_A), "sealed_keyset": h(&sealed),
                 "sym_key": h(&plaintext[1..33]), "vault_id": h(&VAULT), "vault_key": h(&[0x34; 32]),
                 "wrapped": h(&wrapped.to_bytes()), "probe_item_id": h(&ITEM), "probe_revision": "1",
                 "probe_plaintext": h(b"probe item"), "probe_envelope": h(&item.to_bytes()) }),
    ]
}

fn signature() -> Vec<Value> {
    let auk = key(0x40);
    let (keyset, sealed, plaintext) = known_keyset(&auk, &ACCOUNT_A, 0x41, 0x42, 0x43);
    let messages: [&[u8]; 3] = [b"", b"message", &[0xAB; 200]];
    messages
        .iter()
        .map(|msg| {
            json!({ "auk": h(&[0x40; 32]), "account_id": h(&ACCOUNT_A), "sealed_keyset": h(&sealed),
                    "ed25519_seed": h(&plaintext[65..97]), "ed25519_public": h(&keyset.ed25519_public()),
                    "context": "share", "label": "pwm/share-sig", "message": h(msg),
                    "signature": h(&keyset.sign(SignatureContext::Share, msg)) })
        })
        .collect()
}

fn share() -> Vec<Value> {
    let (sender, sender_sealed, sender_pt) = known_keyset(&key(0x50), &ACCOUNT_A, 0x51, 0x52, 0x53);
    let (recipient, recipient_sealed, recipient_pt) =
        known_keyset(&key(0x60), &ACCOUNT_B, 0x61, 0x62, 0x63);
    let vk = key(0x70);
    let share = sender
        .share_vault_key(&vk, &VAULT, &recipient.x25519_public())
        .unwrap();
    recipient
        .accept_vault_share(&share, &VAULT, &sender.ed25519_public())
        .unwrap();
    // Exercise the account API once more so the generator fails loudly if it drifts.
    let _ = create_account("generator self-check", &ACCOUNT_A, KdfParams::MOBILE_FLOOR).unwrap();
    vec![json!({
        "vault_id": h(&VAULT), "vault_key": h(&[0x70; 32]),
        "sender": { "auk": h(&[0x50; 32]), "account_id": h(&ACCOUNT_A), "sealed_keyset": h(&sender_sealed),
                    "ed25519_seed": h(&sender_pt[65..97]), "ed25519_public": h(&sender.ed25519_public()) },
        "recipient": { "auk": h(&[0x60; 32]), "account_id": h(&ACCOUNT_B), "sealed_keyset": h(&recipient_sealed),
                       "x25519_secret": h(&recipient_pt[33..65]), "x25519_public": h(&recipient.x25519_public()) },
        "share": h(&share.to_bytes()),
    })]
}

fn export() -> Vec<Value> {
    let cases: [(&str, KdfParams, [u8; 16], &[u8]); 2] = [
        (
            "orbit velvet candle harbor ninth",
            KdfParams::DEFAULT,
            [0x3C; 16],
            br#"{"format":"pwm-export","version":1,"exported_at":0,"vaults":[]}"#,
        ),
        (
            "caf\u{e9}\u{a0}export",
            KdfParams::MOBILE_FLOOR,
            [0xC3; 16],
            b"",
        ),
    ];
    cases
        .iter()
        .map(|(password, params, salt, plaintext)| {
            let normalized = normalize_password(password).unwrap();
            let pw_key = derive_password_key(normalized.expose_secret().as_bytes(), salt, params).unwrap();
            let export_key = hkdf_sha256(pw_key.expose_secret(), salt, b"pwm/export/v1");
            let aad = Aad::export(1, params, salt);
            let key = SymmetricKey::from_secret(Secret::new(*export_key.expose_secret()));
            let envelope = seal(&key, plaintext, &aad).unwrap();
            let mut file = b"PWMX\x01".to_vec();
            for n in [params.memory_kib, params.iterations, params.parallelism] {
                file.extend_from_slice(&n.to_be_bytes());
            }
            file.extend_from_slice(salt);
            file.extend_from_slice(&envelope.to_bytes());
            // Cross-check against the high-level API.
            assert_eq!(open_export(password, &file).unwrap().expose_secret().as_slice(), *plaintext);

            json!({ "password": h(password.as_bytes()), "params": params_json(params), "salt": h(salt),
                    "pw_key": h(pw_key.expose_secret()), "export_key": h(export_key.expose_secret()),
                    "aad": h(aad.as_bytes()), "plaintext": h(plaintext), "file": h(&file) })
        })
        .collect()
}

fn device_name() -> Vec<Value> {
    let auk = key(0x40);
    let (keyset, sealed, plaintext) = known_keyset(&auk, &ACCOUNT_A, 0x41, 0x42, 0x43);
    ["Phone", &"\u{e9}".repeat(31), ""]
        .iter()
        .map(|name| {
            let envelope = keyset.seal_device_name(&ACCOUNT_A, &DEVICE, name).unwrap();
            assert_eq!(keyset.open_device_name(&envelope, &ACCOUNT_A, &DEVICE).unwrap(), *name);
            let mut padded = [0u8; 64];
            padded[..name.len()].copy_from_slice(name.as_bytes());
            padded[name.len()] = 0x80;
            json!({ "auk": h(&[0x40; 32]), "account_id": h(&ACCOUNT_A), "sealed_keyset": h(&sealed),
                    "sym_key": h(&plaintext[1..33]), "device_id": h(&DEVICE), "name": h(name.as_bytes()),
                    "aad": h(Aad::device_name(1, &ACCOUNT_A, &DEVICE).as_bytes()),
                    "padded": h(&padded), "envelope": h(&envelope.to_bytes()) })
        })
        .collect()
}

/// The session blob (keyset under a random session key, `pwm/session`) and
/// a device secret under `sym_key` (`pwm/device-tokens`).
fn session() -> Vec<Value> {
    let auk = key(0x44);
    let (keyset, sealed, plaintext) = known_keyset(&auk, &ACCOUNT_A, 0x45, 0x46, 0x47);
    let (blob, session_key) = keyset.export_session(&ACCOUNT_A).unwrap();
    let secret = b"pwmr_example-refresh-token";
    let sealed_secret = keyset.seal_device_secret(&ACCOUNT_A, secret).unwrap();
    vec![json!({
        "auk": h(&[0x44; 32]), "account_id": h(&ACCOUNT_A), "sealed_keyset": h(&sealed),
        "keyset_plaintext": h(&plaintext), "session_key": h(session_key.expose_secret()),
        "session_aad": h(Aad::session(1, &ACCOUNT_A).as_bytes()), "session_blob": h(&blob.to_bytes()),
        "sym_key": h(&plaintext[1..33]), "device_secret": h(secret),
        "device_tokens_aad": h(Aad::device_tokens(1, &ACCOUNT_A).as_bytes()),
        "device_secret_envelope": h(&sealed_secret.to_bytes()),
    })]
}

/// The biometric unlock blob: the keyset under a random key,
/// `pwm/biometric-unlock`, bound to the account and the device (D45).
fn biometric_unlock() -> Vec<Value> {
    let auk = key(0x48);
    let (keyset, sealed, plaintext) = known_keyset(&auk, &ACCOUNT_A, 0x49, 0x4A, 0x4B);
    let (envelope, unlock_key) = keyset.export_biometric_unlock(&ACCOUNT_A, &DEVICE).unwrap();
    vec![json!({
        "auk": h(&[0x48; 32]), "account_id": h(&ACCOUNT_A), "device_id": h(&DEVICE),
        "sealed_keyset": h(&sealed), "keyset_plaintext": h(&plaintext),
        "unlock_key": h(unlock_key.expose_secret()),
        "aad": h(Aad::biometric_unlock(1, &ACCOUNT_A, &DEVICE).as_bytes()),
        "envelope": h(&envelope.to_bytes()),
    })]
}

/// The browser unlock blob: the keyset under a random key,
/// `pwm/browser-unlock`, bound to the account and the device (D48 §5).
fn browser_unlock() -> Vec<Value> {
    let auk = key(0x58);
    let (keyset, sealed, plaintext) = known_keyset(&auk, &ACCOUNT_A, 0x59, 0x5A, 0x5B);
    let (envelope, unlock_key) = keyset.export_browser_unlock(&ACCOUNT_A, &DEVICE).unwrap();
    vec![json!({
        "auk": h(&[0x58; 32]), "account_id": h(&ACCOUNT_A), "device_id": h(&DEVICE),
        "sealed_keyset": h(&sealed), "keyset_plaintext": h(&plaintext),
        "unlock_key": h(unlock_key.expose_secret()),
        "aad": h(Aad::browser_unlock(1, &ACCOUNT_A, &DEVICE).as_bytes()),
        "envelope": h(&envelope.to_bytes()),
    })]
}

/// Device approval (D46): fixed key seeds on both sides, the commitment,
/// the code, and the Secret Key sealed by the signed-in device.
fn approval() -> Vec<Value> {
    let approval_id: Id = [0xAB; 16];
    let new = ApprovalKeys::from_seed(&Secret::new([0x5A; 32]));
    let existing = ApprovalKeys::from_seed(&Secret::new([0x5B; 32]));
    let (n_pub, e_pub) = (new.public(), existing.public());
    let channel = existing.channel(&approval_id, &n_pub, &e_pub).unwrap();
    let at_new = new.channel(&approval_id, &n_pub, &e_pub).unwrap();
    assert_eq!(channel.code(), at_new.code());
    let secret_key = b"A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA";
    let envelope = channel.seal(secret_key).unwrap();
    vec![json!({
        "approval_id": h(&approval_id),
        "new_secret": h(&[0x5A; 32]), "new_public": h(&n_pub),
        "existing_secret": h(&[0x5B; 32]), "existing_public": h(&e_pub),
        "commitment": h(&approval_commitment(&n_pub)),
        "code": channel.code(), "code_text": channel.code_text(),
        "aad": h(Aad::approval(1, &approval_id, &n_pub, &e_pub).as_bytes()),
        "secret_key": String::from_utf8(secret_key.to_vec()).unwrap(),
        "envelope": h(&envelope.to_bytes()),
    })]
}

/// One registration and login with seeded randomness. The checker replays
/// the server's deterministic steps against the recorded messages and logs
/// in again with the frozen record, so an `opaque-ke` upgrade or a changed
/// setting that would lock existing accounts out fails the build.
fn opaque() -> Vec<Value> {
    let mut rng = ChaCha20Rng::from_seed([0x07; 32]);
    let auth_secret = Secret::new([0x51; 32]);
    let server = ServerSecrets::generate_with_rng(&mut rng).unwrap();
    let (registration, registration_request) =
        OpaqueRegistration::start_with_rng(&mut rng, &auth_secret).unwrap();
    let registration_response = server
        .register_respond(&ACCOUNT_A, &registration_request)
        .unwrap();
    let registration_upload = registration
        .finish_with_rng(&mut rng, &auth_secret, &ACCOUNT_A, &registration_response)
        .unwrap();
    let record = opaque_record(&registration_upload).unwrap();

    let (login, credential_request) = OpaqueLogin::start_with_rng(&mut rng, &auth_secret).unwrap();
    let (state, credential_response) = server
        .login_respond_with_rng(&mut rng, Some(&record), &ACCOUNT_A, &credential_request)
        .unwrap();
    let credential_finalization = login
        .finish_with_rng(&mut rng, &auth_secret, &ACCOUNT_A, &credential_response)
        .unwrap();
    server
        .login_finish(&state, &credential_finalization, &ACCOUNT_A)
        .unwrap();

    let (fake_id, fake_salt) = server.prelogin_fake("a@example.com");
    vec![json!({
        "suite": "OPRF ristretto255-SHA512, 3DH ristretto255 + SHA-512, KSF Identity",
        "context": h(b"pwm/opaque/v1"), "rng_seed": h(&[0x07; 32]),
        "server_secrets": h(server.to_bytes().expose_secret()),
        "account_id": h(&ACCOUNT_A), "auth_secret": h(&[0x51; 32]),
        "registration_request": h(&registration_request), "registration_response": h(&registration_response),
        "registration_upload": h(&registration_upload), "record": h(&record),
        "credential_request": h(&credential_request), "credential_response": h(&credential_response),
        "server_state_sealed": h(&state), "credential_finalization": h(&credential_finalization),
        "prelogin_email": "a@example.com", "prelogin_fake_id": h(&fake_id), "prelogin_fake_salt": h(&fake_salt),
    })]
}

fn windows_hello() -> Vec<Value> {
    // An RSA-2048 PKCS#1 v1.5 signature stands in as fixed bytes: only its
    // bytes enter HKDF.
    let signature: Vec<u8> = (0..=255u8).collect();
    let challenge = [0x5C; 32];
    let unlock_key = [0x77; 32];
    let sealed = pwm_crypto::seal_windows_hello(
        &Secret::new(unlock_key),
        &signature,
        &Secret::new(challenge),
        &ACCOUNT_A,
        &DEVICE,
    )
    .unwrap();
    let wrapping_key = pwm_crypto::hkdf_sha256(&signature, &challenge, b"pwm/windows-hello/v1");
    vec![json!({
        "account_id": h(&ACCOUNT_A), "device_id": h(&DEVICE),
        "signature": h(&signature), "challenge": h(&challenge),
        "info": h(b"pwm/windows-hello/v1"),
        "wrapping_key": h(wrapping_key.expose_secret()),
        "unlock_key": h(&unlock_key),
        "aad": h(Aad::windows_hello(1, &ACCOUNT_A, &DEVICE).as_bytes()),
        "envelope": h(&sealed.to_bytes()),
    })]
}
