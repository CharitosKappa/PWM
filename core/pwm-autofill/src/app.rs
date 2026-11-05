//! Matching a login to an Android app (D58 §4). A login's website list can
//! hold `android-app://<package>?sha256=<digest>`: the app's package name and
//! the SHA-256 of its signing certificate, which the platform reads from the
//! system. Both must be equal, so an app installed later under the same name
//! from elsewhere doesn't get the login. Nothing is guessed from the name.

/// How a login's link matches the app asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMatch {
    /// The package and one of its certificates: it may be suggested.
    Linked,
    /// The package only, from a link with no digest (another manager's
    /// export): offered from search, never suggested.
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AppLinkError {
    #[error("not an Android package name")]
    Package,
    #[error("not a SHA-256 digest")]
    Digest,
}

const SCHEME: &str = "android-app://";
/// The form other managers export, without a certificate.
const LEGACY_SCHEME: &str = "androidapp://";

/// The link to store for this app: `android-app://<package>?sha256=<hex>`.
pub fn app_link(package: &str, cert_sha256: &str) -> Result<String, AppLinkError> {
    if !is_package_name(package) {
        return Err(AppLinkError::Package);
    }
    let digest = digest(cert_sha256).ok_or(AppLinkError::Digest)?;
    Ok(format!("{SCHEME}{package}?sha256={digest}"))
}

/// Whether the login's URL `item` is a link to the app `package`, signed with
/// one of `certs` (SHA-256 digests, with or without colons).
pub fn match_app(package: &str, certs: &[String], item: &str) -> Option<AppMatch> {
    if !is_package_name(package) {
        return None;
    }
    if let Some(linked) = item.strip_prefix(LEGACY_SCHEME) {
        return (linked == package).then_some(AppMatch::Unverified);
    }
    let rest = item.strip_prefix(SCHEME)?;
    let (linked, query) = match rest.split_once('?') {
        Some((linked, query)) => (linked, Some(query)),
        None => (rest, None),
    };
    if linked != package {
        return None;
    }
    let Some(query) = query else {
        return Some(AppMatch::Unverified);
    };
    let wanted = digest(query.strip_prefix("sha256=")?)?;
    certs
        .iter()
        .filter_map(|c| digest(c))
        .any(|c| c == wanted)
        .then_some(AppMatch::Linked)
}

/// A Java-style package: at least two parts, each a letter then letters,
/// digits or underscores.
pub fn is_package_name(package: &str) -> bool {
    let parts: Vec<&str> = package.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            let mut chars = p.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// 64 hex digits, lowercase, colons dropped.
fn digest(text: &str) -> Option<String> {
    let hex: String = text
        .chars()
        .filter(|c| *c != ':')
        .collect::<String>()
        .to_ascii_lowercase();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}
