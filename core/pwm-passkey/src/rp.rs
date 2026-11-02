//! Which relying party ids a page may use (WebAuthn §5.1.3 step 8, §5.1.4):
//! the origin's host, or a registrable suffix of it, never a public suffix
//! (Public Suffix List, D33) and never another site.

use pwm_autofill::registrable_domain;

use crate::PasskeyError;

fn is_localhost(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost")
}

/// HTTPS, or HTTP on `localhost` (secure contexts that WebAuthn allows).
pub fn is_secure_origin(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    match (url.scheme(), url.host_str()) {
        ("https", Some(_)) => true,
        ("http", Some(host)) => is_localhost(host),
        _ => false,
    }
}

/// The RP id the request may use on `origin`: `requested`, or the origin's
/// host when the page gave none.
pub fn effective_rp_id(origin: &str, requested: Option<&str>) -> Result<String, PasskeyError> {
    if !is_secure_origin(origin) {
        return Err(PasskeyError::InsecureOrigin);
    }
    let url = url::Url::parse(origin).map_err(|_| PasskeyError::InsecureOrigin)?;
    let host = match url.host() {
        Some(url::Host::Domain(host)) => host.to_ascii_lowercase(),
        // WebAuthn RP ids are domains; IP-address origins get none.
        _ => return Err(PasskeyError::InvalidRpId),
    };
    let rp_id = match requested {
        None => return Ok(host),
        Some(id) => id.trim_end_matches('.').to_ascii_lowercase(),
    };
    if rp_id.is_empty() || rp_id.contains(['/', ':', ' ']) {
        return Err(PasskeyError::InvalidRpId);
    }
    let belongs = host == rp_id || host.ends_with(&format!(".{rp_id}"));
    if !belongs {
        return Err(PasskeyError::InvalidRpId);
    }
    // `localhost` has no registrable domain but is its own RP id.
    if is_localhost(&rp_id) {
        return Ok(rp_id);
    }
    // A public suffix ("co.uk", "github.io") is never an RP id.
    match registrable_domain(&rp_id) {
        Some(domain) if rp_id.ends_with(&domain) => Ok(rp_id),
        _ => Err(PasskeyError::InvalidRpId),
    }
}
