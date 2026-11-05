//! Which relying party ids a page may use (WebAuthn §5.1.3 step 8, §5.1.4):
//! the origin's host, or a registrable suffix of it, never a public suffix
//! (Public Suffix List, D33) and never another site.

use pwm_autofill::registrable_domain;

use crate::PasskeyError;

/// A name DNS can hold (RFC 1035 §2.3.4): at most 253 characters in its
/// ASCII form, and 1 to 63 in each label. The request's own limit is on the
/// text as given, and an IDNA name can grow past it once converted; a passkey
/// for such a name could never be asked for again.
fn fits_dns(name: &str) -> bool {
    name.len() <= 253 && name.split('.').all(|l| !l.is_empty() && l.len() <= 63)
}

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
        Some(url::Host::Domain(host)) if fits_dns(host) => host.to_ascii_lowercase(),
        // WebAuthn RP ids are domains; IP-address origins get none.
        _ => return Err(PasskeyError::InvalidRpId),
    };
    let rp_id = match requested {
        None => return Ok(host),
        Some(id) => checked_rp_id(id)?,
    };
    let belongs = host == rp_id || host.ends_with(&format!(".{rp_id}"));
    if !belongs {
        return Err(PasskeyError::InvalidRpId);
    }
    Ok(rp_id)
}

/// An RP id given without a page, as a platform passes it after checking
/// the app's or site's associated domains (D47 §6): lowercased, and a
/// registrable domain or a name under one (or `localhost`), never a public
/// suffix or an IP address.
pub fn checked_rp_id(requested: &str) -> Result<String, PasskeyError> {
    let rp_id = requested.trim_end_matches('.').to_ascii_lowercase();
    if rp_id.is_empty() || rp_id.contains(['/', ':', ' ']) {
        return Err(PasskeyError::InvalidRpId);
    }
    // The ASCII (IDNA) form; IP addresses are never RP ids.
    let rp_id = match url::Host::parse(&rp_id) {
        Ok(url::Host::Domain(domain)) if fits_dns(&domain) => domain,
        _ => return Err(PasskeyError::InvalidRpId),
    };
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
