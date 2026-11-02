//! WebAuthn requests as the extension passes them: the page's options in
//! their JSON form (binary values as base64url), parsed with limits. Every
//! value here came from a web page and is untrusted.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use crate::PasskeyError;

/// COSE algorithm id of ES256 (ECDSA P-256 with SHA-256), the only one here.
pub const ES256: i64 = -7;

const MAX_CHALLENGE: usize = 1024;
const MAX_USER_ID: usize = 64;
const MAX_CREDENTIAL_ID: usize = 1023;
const MAX_TEXT: usize = 256;
const MAX_LIST: usize = 64;

/// Base64url as WebAuthn's JSON uses it; padding is tolerated.
pub(crate) fn b64(text: &str) -> Result<Vec<u8>, PasskeyError> {
    URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .map_err(|_| PasskeyError::InvalidRequest)
}

pub(crate) fn b64_text(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn limited(text: &str) -> Result<String, PasskeyError> {
    if text.chars().count() > MAX_TEXT {
        return Err(PasskeyError::InvalidRequest);
    }
    Ok(text.to_owned())
}

fn bounded(bytes: Vec<u8>, min: usize, max: usize) -> Result<Vec<u8>, PasskeyError> {
    if bytes.len() < min || bytes.len() > max {
        return Err(PasskeyError::InvalidRequest);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCredential {
    #[serde(rename = "type", default)]
    kind: String,
    id: String,
}

fn credentials(list: Option<Vec<RawCredential>>) -> Result<Vec<Vec<u8>>, PasskeyError> {
    let list = list.unwrap_or_default();
    if list.len() > MAX_LIST {
        return Err(PasskeyError::InvalidRequest);
    }
    list.into_iter()
        .filter(|c| c.kind == "public-key")
        .map(|c| bounded(b64(&c.id)?, 1, MAX_CREDENTIAL_ID))
        .collect()
}

#[derive(Deserialize)]
struct RawRp {
    id: Option<String>,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUser {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
}

#[derive(Deserialize)]
struct RawParam {
    #[serde(rename = "type", default)]
    kind: String,
    alg: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCreate {
    challenge: String,
    rp: RawRp,
    user: RawUser,
    #[serde(default)]
    pub_key_cred_params: Vec<RawParam>,
    exclude_credentials: Option<Vec<RawCredential>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGet {
    challenge: String,
    rp_id: Option<String>,
    allow_credentials: Option<Vec<RawCredential>>,
}

/// `navigator.credentials.create({publicKey})`, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRequest {
    pub challenge: Vec<u8>,
    /// The page's RP id, if it gave one (else the origin's host).
    pub rp_id: Option<String>,
    pub rp_name: String,
    pub user_id: Vec<u8>,
    pub user_name: String,
    pub user_display_name: String,
    /// ES256 is among the algorithms the site accepts.
    pub es256: bool,
    pub exclude: Vec<Vec<u8>>,
}

impl CreateRequest {
    pub fn from_json(json: &str) -> Result<Self, PasskeyError> {
        let raw: RawCreate =
            serde_json::from_str(json).map_err(|_| PasskeyError::InvalidRequest)?;
        if raw.pub_key_cred_params.len() > MAX_LIST {
            return Err(PasskeyError::InvalidRequest);
        }
        // An empty list means the defaults, which include ES256 (WebAuthn §5.4).
        let es256 = raw.pub_key_cred_params.is_empty()
            || raw
                .pub_key_cred_params
                .iter()
                .any(|p| p.kind == "public-key" && p.alg == ES256);
        Ok(Self {
            challenge: bounded(b64(&raw.challenge)?, 1, MAX_CHALLENGE)?,
            rp_id: raw.rp.id.map(|id| limited(&id)).transpose()?,
            rp_name: limited(&raw.rp.name)?,
            user_id: bounded(b64(&raw.user.id)?, 1, MAX_USER_ID)?,
            user_name: limited(&raw.user.name)?,
            user_display_name: limited(&raw.user.display_name)?,
            es256,
            exclude: credentials(raw.exclude_credentials)?,
        })
    }
}

/// `navigator.credentials.get({publicKey})`, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetRequest {
    pub challenge: Vec<u8>,
    pub rp_id: Option<String>,
    /// Empty: any of the site's passkeys (discoverable).
    pub allow: Vec<Vec<u8>>,
}

impl GetRequest {
    pub fn from_json(json: &str) -> Result<Self, PasskeyError> {
        let raw: RawGet = serde_json::from_str(json).map_err(|_| PasskeyError::InvalidRequest)?;
        Ok(Self {
            challenge: bounded(b64(&raw.challenge)?, 1, MAX_CHALLENGE)?,
            rp_id: raw.rp_id.map(|id| limited(&id)).transpose()?,
            allow: credentials(raw.allow_credentials)?,
        })
    }
}
