//! Digital Asset Links (D61 §4): whether a site's
//! `/.well-known/assetlinks.json` lists an Android app, so the app may use
//! the site's passkeys. The platform fetches the file; parsing and matching
//! it happen here. The file is the site's, and still untrusted input: it is
//! read with limits, and anything malformed lists nothing.
//!
//! A statement counts when its target is `android_app` with the app's
//! package name and its certificate's SHA-256 among the fingerprints, and
//! its relations include one of [`LOGIN_RELATIONS`]. `include` statements
//! name other files to read; the platform fetches those one level deep.

use serde_json::Value;

use crate::{PasskeyError, checked_rp_id};

/// The largest file read (D61 §4).
pub const MAX_ASSET_LINKS: usize = 256 * 1024;

/// At most this many `include` statements are followed.
pub const MAX_INCLUDES: usize = 10;

/// The relations that let an app use the site's sign-ins: credential sharing,
/// and app links, which many sites list alone.
pub const LOGIN_RELATIONS: [&str; 2] = [
    "delegate_permission/common.get_login_creds",
    "delegate_permission/common.handle_all_urls",
];

/// What one file says about an app.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetLinks {
    /// The file lists the app for sign-ins.
    pub linked: bool,
    /// Other files it includes: HTTPS on a domain, each once, at most
    /// [`MAX_INCLUDES`].
    pub includes: Vec<String>,
}

/// The file to fetch for an RP id.
pub fn asset_links_url(rp_id: &str) -> Result<String, PasskeyError> {
    let rp_id = checked_rp_id(rp_id)?;
    Ok(format!("https://{rp_id}/.well-known/assetlinks.json"))
}

/// Reads a file for the app `package` signed with `cert_sha256`.
pub fn read_asset_links(
    text: &str,
    package: &str,
    cert_sha256: &[u8],
) -> Result<AssetLinks, PasskeyError> {
    let statements = statements(text)?;
    let linked = statements
        .iter()
        .any(|s| lists_app(s, package, cert_sha256));
    Ok(AssetLinks {
        linked,
        includes: includes(&statements),
    })
}

/// The files a file includes, for the platform to fetch.
pub fn asset_links_includes(text: &str) -> Result<Vec<String>, PasskeyError> {
    Ok(includes(&statements(text)?))
}

fn statements(text: &str) -> Result<Vec<Value>, PasskeyError> {
    if text.len() > MAX_ASSET_LINKS {
        return Err(PasskeyError::InvalidAssetLinks);
    }
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else {
        return Err(PasskeyError::InvalidAssetLinks);
    };
    if list.iter().any(|s| !s.is_object()) {
        return Err(PasskeyError::InvalidAssetLinks);
    }
    Ok(list)
}

fn lists_app(statement: &Value, package: &str, cert_sha256: &[u8]) -> bool {
    let relations = statement["relation"].as_array();
    let login = relations.is_some_and(|r| {
        r.iter()
            .filter_map(Value::as_str)
            .any(|r| LOGIN_RELATIONS.contains(&r))
    });
    let target = &statement["target"];
    login
        && target["namespace"] == "android_app"
        && target["package_name"] == package
        && target["sha256_cert_fingerprints"]
            .as_array()
            .is_some_and(|prints| {
                prints
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(fingerprint)
                    .any(|p| p == cert_sha256)
            })
}

/// `AB:CD:…` (or the same without colons, any case) as 32 bytes.
fn fingerprint(text: &str) -> Option<Vec<u8>> {
    let hex: String = text.chars().filter(|c| *c != ':').collect();
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..32)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

fn includes(statements: &[Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in statements {
        let Some(text) = s["include"].as_str() else {
            continue;
        };
        let Ok(url) = url::Url::parse(text) else {
            continue;
        };
        let on_domain = matches!(url.host(), Some(url::Host::Domain(_)));
        if url.scheme() != "https" || !on_domain || url.username() != "" || url.password().is_some()
        {
            continue;
        }
        let url = url.to_string();
        if !out.contains(&url) {
            out.push(url);
        }
        if out.len() == MAX_INCLUDES {
            break;
        }
    }
    out
}
