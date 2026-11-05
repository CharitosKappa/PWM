//! Passkeys for Android's Credential Manager (D61 §2-3). Two callers reach a
//! provider:
//! - a privileged browser, whose origin Android vouches for and which builds
//!   the client data itself, handing over only its SHA-256 hash; the
//!   response carries a placeholder where `clientDataJSON` goes
//! - any other app, for which the client data is built here, with the origin
//!   `android:apk-key-hash:<base64url SHA-256 of its signing certificate>`
//!   and its package name, as Google's own provider writes it
//!
//! An app has no origin to check the RP id against: the site's Digital Asset
//! Links file must list it (`asset_links`), which the caller checks before
//! these functions run.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_autofill::is_package_name;
use pwm_crypto::Secret;
use serde_json::json;

use crate::request::{self, CreateRequest, GetRequest};
use crate::{
    Ceremony, PasskeyError, Registration, StoredPasskey, assert, checked_rp_id, create,
    effective_rp_id, sha256, with_fresh_key,
};

/// The origin scheme of an Android app's client data.
pub const APK_KEY_HASH: &str = "android:apk-key-hash:";

/// What the response carries in place of `clientDataJSON` when a browser
/// built it (D61 §2): base64url of `{}`, so the response still parses.
pub const CLIENT_DATA_PLACEHOLDER: &str = "e30";

/// Who asks through the Credential Manager (D61 §1).
#[derive(Debug, Clone, Copy)]
pub enum AndroidCaller<'a> {
    /// A browser on the privileged list, with the origin Android returned
    /// for it and the hash of the client data it built.
    Browser {
        origin: &'a str,
        client_data_hash: &'a [u8],
    },
    /// Any other app, by package and the SHA-256 of its signing certificate,
    /// already checked against the RP's Digital Asset Links.
    App {
        package: &'a str,
        cert_sha256: &'a [u8],
    },
}

/// `android:apk-key-hash:<base64url, no padding>` for a certificate digest.
pub fn apk_key_hash_origin(cert_sha256: &[u8]) -> Result<String, PasskeyError> {
    if cert_sha256.len() != 32 {
        return Err(PasskeyError::InvalidRequest);
    }
    Ok(format!(
        "{APK_KEY_HASH}{}",
        URL_SAFE_NO_PAD.encode(cert_sha256)
    ))
}

/// An app's `clientDataJSON`: type, challenge, origin and package, in the
/// order Google's provider writes them.
pub fn app_client_data_json(
    ceremony: Ceremony,
    challenge: &[u8],
    origin: &str,
    package: &str,
) -> Vec<u8> {
    let kind = match ceremony {
        Ceremony::Create => "webauthn.create",
        Ceremony::Get => "webauthn.get",
    };
    format!(
        r#"{{"type":"{kind}","challenge":"{}","origin":{},"androidPackageName":{}}}"#,
        request::b64_text(challenge),
        json!(origin),
        json!(package)
    )
    .into_bytes()
}

/// The RP id a request may use: for a browser, checked against its origin
/// as for a page; for an app, the one it names (it must name one), checked
/// as any RP id given without a page.
pub fn android_rp_id(
    caller: &AndroidCaller<'_>,
    requested: Option<&str>,
) -> Result<String, PasskeyError> {
    match caller {
        AndroidCaller::Browser { origin, .. } => effective_rp_id(origin, requested),
        AndroidCaller::App { package, .. } => {
            if !is_package_name(package) {
                return Err(PasskeyError::InvalidRequest);
            }
            checked_rp_id(requested.ok_or(PasskeyError::InvalidRequest)?)
        }
    }
}

/// The response's `clientDataJSON` field and the hash that is signed.
fn client_data(
    caller: &AndroidCaller<'_>,
    ceremony: Ceremony,
    challenge: &[u8],
) -> Result<(String, [u8; 32]), PasskeyError> {
    match caller {
        AndroidCaller::Browser {
            client_data_hash, ..
        } => {
            let hash: [u8; 32] = (*client_data_hash)
                .try_into()
                .map_err(|_| PasskeyError::InvalidRequest)?;
            Ok((CLIENT_DATA_PLACEHOLDER.to_owned(), hash))
        }
        AndroidCaller::App {
            package,
            cert_sha256,
        } => {
            let origin = apk_key_hash_origin(cert_sha256)?;
            let data = app_client_data_json(ceremony, challenge, &origin, package);
            Ok((request::b64_text(&data), sha256(&data)))
        }
    }
}

/// Creates a passkey for a Credential Manager request. `key_seed` and
/// `credential_id` are the randomness, as in [`crate::register`].
pub fn register_android(
    caller: AndroidCaller<'_>,
    request: &CreateRequest,
    key_seed: &Secret<[u8; 32]>,
    credential_id: [u8; 16],
) -> Result<Registration, PasskeyError> {
    let rp_id = android_rp_id(&caller, request.rp_id.as_deref())?;
    // With the `none` attestation nothing signs the client data at
    // registration; a browser's hash is still checked for its length.
    let (field, _) = client_data(&caller, Ceremony::Create, &request.challenge)?;
    create(rp_id, request, key_seed, credential_id, &field)
}

/// [`register_android`] with fresh randomness.
pub fn register_android_random(
    caller: AndroidCaller<'_>,
    request: &CreateRequest,
) -> Result<Registration, PasskeyError> {
    with_fresh_key(|seed, id| register_android(caller, request, seed, id))
}

/// Signs a Credential Manager sign-in with a stored passkey:
/// `AuthenticationResponseJSON`.
pub fn authenticate_android(
    caller: AndroidCaller<'_>,
    request: &GetRequest,
    passkey: &StoredPasskey,
) -> Result<String, PasskeyError> {
    let rp_id = android_rp_id(&caller, request.rp_id.as_deref())?;
    let (field, hash) = client_data(&caller, Ceremony::Get, &request.challenge)?;
    assert(&rp_id, request, passkey, &field, &hash)
}
