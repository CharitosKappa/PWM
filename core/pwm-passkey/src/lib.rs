//! Passkeys (decision D37): the authenticator side of WebAuthn Level 3 for
//! ES256 credentials, as the browser extension (and later the apps) act as
//! one. This crate checks the relying party id against the browser-given
//! origin, builds `clientDataJSON` itself, makes and signs with P-256 keys
//! (RustCrypto `p256`), and writes `authenticatorData` and the `none`
//! attestation object. The page's JSON is untrusted (see `request`).
#![forbid(unsafe_code)]

mod cbor;
mod request;
mod rp;

use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey};
use pwm_crypto::Secret;
use serde_json::json;
use sha2::{Digest, Sha256};

pub use request::{CreateRequest, ES256, GetRequest};
pub use rp::{effective_rp_id, is_secure_origin};

/// PWM's authenticator model id, the same on every platform (D37 §3).
pub const AAGUID: [u8; 16] = [
    0x6c, 0x9e, 0x3d, 0x41, 0x8a, 0x27, 0x4f, 0x0b, 0x9d, 0x52, 0xe1, 0x77, 0x30, 0xc4, 0x58, 0xa6,
];

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_BE: u8 = 0x08;
const FLAG_BS: u8 = 0x10;
const FLAG_AT: u8 = 0x40;
/// User present and verified; backup eligible and backed up (synced).
const FLAGS: u8 = FLAG_UP | FLAG_UV | FLAG_BE | FLAG_BS;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PasskeyError {
    #[error("the request is malformed")]
    InvalidRequest,
    #[error("the page is not a secure origin")]
    InsecureOrigin,
    #[error("the relying party id does not belong to the page")]
    InvalidRpId,
    #[error("the site accepts no algorithm this authenticator has")]
    UnsupportedAlgorithm,
    #[error("the stored key is unusable")]
    InvalidKey,
}

/// The ceremony a `clientDataJSON` belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ceremony {
    Create,
    Get,
}

/// `clientDataJSON` (WebAuthn §5.8.1), built from the browser-given origin.
/// `top_origin` is set when the request came from a same-site frame of
/// another origin.
pub fn client_data_json(
    ceremony: Ceremony,
    challenge: &[u8],
    origin: &str,
    top_origin: Option<&str>,
) -> Vec<u8> {
    let kind = match ceremony {
        Ceremony::Create => "webauthn.create",
        Ceremony::Get => "webauthn.get",
    };
    // serde_json escapes the origin; key order is the one the spec lists.
    let mut text = format!(
        r#"{{"type":"{kind}","challenge":"{}","origin":{}"#,
        request::b64_text(challenge),
        json!(origin)
    );
    match top_origin {
        Some(top) => text.push_str(&format!(
            r#","crossOrigin":true,"topOrigin":{}}}"#,
            json!(top)
        )),
        None => text.push_str(r#","crossOrigin":false}"#),
    }
    text.into_bytes()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// `authenticatorData` (§6.1): RP id hash, flags, a sign count of 0 (synced
/// passkeys, D37 §3), and on create the attested credential data.
fn authenticator_data(rp_id: &str, attested: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(37 + attested.map_or(0, <[u8]>::len));
    out.extend_from_slice(&sha256(rp_id.as_bytes()));
    out.push(if attested.is_some() {
        FLAGS | FLAG_AT
    } else {
        FLAGS
    });
    out.extend_from_slice(&0u32.to_be_bytes());
    if let Some(attested) = attested {
        out.extend_from_slice(attested);
    }
    out
}

/// The credential public key as a COSE_Key (RFC 9053 EC2, P-256, ES256).
fn cose_key(x: &[u8], y: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(77);
    cbor::map(&mut out, 5);
    cbor::int(&mut out, 1); // kty
    cbor::int(&mut out, 2); // EC2
    cbor::int(&mut out, 3); // alg
    cbor::int(&mut out, ES256);
    cbor::int(&mut out, -1); // crv
    cbor::int(&mut out, 1); // P-256
    cbor::int(&mut out, -2); // x
    cbor::bytes(&mut out, x);
    cbor::int(&mut out, -3); // y
    cbor::bytes(&mut out, y);
    out
}

/// The attestation object with `fmt: "none"` (§8.7), keys in deterministic order.
fn attestation_object(auth_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(auth_data.len() + 32);
    cbor::map(&mut out, 3);
    cbor::text(&mut out, "fmt");
    cbor::text(&mut out, "none");
    cbor::text(&mut out, "attStmt");
    cbor::map(&mut out, 0);
    cbor::text(&mut out, "authData");
    cbor::bytes(&mut out, auth_data);
    out
}

/// A passkey to store in the vault (D18 fields).
pub struct StoredPasskey {
    pub credential_id: Vec<u8>,
    pub rp_id: String,
    pub user_handle: Vec<u8>,
    pub user_name: String,
    /// PKCS#8 DER.
    pub private_key: Secret<Vec<u8>>,
}

/// What a registration gives the page and the vault.
pub struct Registration {
    /// `RegistrationResponseJSON` (§5.1.8) for the extension to hand back.
    pub response_json: String,
    pub passkey: StoredPasskey,
}

/// Where a request came from, as the browser reported it.
#[derive(Debug, Clone, Copy)]
pub struct Caller<'a> {
    pub origin: &'a str,
    /// The top page's origin when the request came from a same-site frame of
    /// another origin.
    pub top_origin: Option<&'a str>,
}

/// Creates a passkey (D37 §3). `key_seed` and `credential_id` are the
/// randomness, passed in so tests are reproducible; [`register_random`]
/// draws them.
pub fn register(
    caller: Caller<'_>,
    request: &CreateRequest,
    key_seed: &Secret<[u8; 32]>,
    credential_id: [u8; 16],
) -> Result<Registration, PasskeyError> {
    let rp_id = effective_rp_id(caller.origin, request.rp_id.as_deref())?;
    if !request.es256 {
        return Err(PasskeyError::UnsupportedAlgorithm);
    }
    let key =
        SigningKey::from_slice(key_seed.expose_secret()).map_err(|_| PasskeyError::InvalidKey)?;
    let point = key.verifying_key().to_sec1_point(false);
    let (Some(x), Some(y)) = (point.x(), point.y()) else {
        return Err(PasskeyError::InvalidKey);
    };
    let cose = cose_key(x, y);
    let mut attested = Vec::with_capacity(18 + credential_id.len() + cose.len());
    attested.extend_from_slice(&AAGUID);
    attested.extend_from_slice(&(credential_id.len() as u16).to_be_bytes());
    attested.extend_from_slice(&credential_id);
    attested.extend_from_slice(&cose);
    let auth_data = authenticator_data(&rp_id, Some(&attested));
    let client_data = client_data_json(
        Ceremony::Create,
        &request.challenge,
        caller.origin,
        caller.top_origin,
    );
    let spki = key
        .verifying_key()
        .to_public_key_der()
        .map_err(|_| PasskeyError::InvalidKey)?;
    let pkcs8 = key.to_pkcs8_der().map_err(|_| PasskeyError::InvalidKey)?;
    let id = request::b64_text(&credential_id);
    let response = json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "authenticatorAttachment": "platform",
        "clientExtensionResults": {},
        "response": {
            "clientDataJSON": request::b64_text(&client_data),
            "attestationObject": request::b64_text(&attestation_object(&auth_data)),
            "authenticatorData": request::b64_text(&auth_data),
            "publicKey": request::b64_text(spki.as_bytes()),
            "publicKeyAlgorithm": ES256,
            "transports": ["internal", "hybrid"],
        },
    });
    Ok(Registration {
        response_json: response.to_string(),
        passkey: StoredPasskey {
            credential_id: credential_id.to_vec(),
            rp_id,
            user_handle: request.user_id.clone(),
            user_name: request.user_name.clone(),
            private_key: Secret::new(pkcs8.as_bytes().to_vec()),
        },
    })
}

/// [`register`] with fresh randomness.
pub fn register_random(
    caller: Caller<'_>,
    request: &CreateRequest,
) -> Result<Registration, PasskeyError> {
    // A uniformly random 32-byte string is a valid P-256 scalar except with
    // negligible probability; retry in that case.
    for _ in 0..4 {
        let seed = pwm_crypto::random_seed().map_err(|_| PasskeyError::InvalidKey)?;
        let id = pwm_crypto::generate_id().map_err(|_| PasskeyError::InvalidKey)?;
        match register(caller, request, &seed, id) {
            Err(PasskeyError::InvalidKey) => continue,
            other => return other,
        }
    }
    Err(PasskeyError::InvalidKey)
}

/// Signs a `get` with a stored passkey (§6.3.3): `AuthenticationResponseJSON`.
pub fn authenticate(
    caller: Caller<'_>,
    request: &GetRequest,
    passkey: &StoredPasskey,
) -> Result<String, PasskeyError> {
    let rp_id = effective_rp_id(caller.origin, request.rp_id.as_deref())?;
    if rp_id != passkey.rp_id {
        return Err(PasskeyError::InvalidRpId);
    }
    if !request.allow.is_empty() && !request.allow.contains(&passkey.credential_id) {
        return Err(PasskeyError::InvalidRequest);
    }
    let key = SigningKey::from_pkcs8_der(passkey.private_key.expose_secret())
        .map_err(|_| PasskeyError::InvalidKey)?;
    let auth_data = authenticator_data(&rp_id, None);
    let client_data = client_data_json(
        Ceremony::Get,
        &request.challenge,
        caller.origin,
        caller.top_origin,
    );
    let mut signed = auth_data.clone();
    signed.extend_from_slice(&sha256(&client_data));
    let signature: Signature = key.sign(&signed);
    let id = request::b64_text(&passkey.credential_id);
    Ok(json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "authenticatorAttachment": "platform",
        "clientExtensionResults": {},
        "response": {
            "clientDataJSON": request::b64_text(&client_data),
            "authenticatorData": request::b64_text(&auth_data),
            "signature": request::b64_text(signature.to_der().as_bytes()),
            "userHandle": request::b64_text(&passkey.user_handle),
        },
    })
    .to_string())
}
