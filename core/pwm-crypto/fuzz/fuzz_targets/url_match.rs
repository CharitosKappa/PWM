//! Autofill URL matching (D33): never panics on any page URL, item URL or
//! pattern, in every mode, and the PSL lookup accepts any host text. Android
//! app links (D58 §4) parse any text too.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_autofill::{app_link, match_app, match_url, registrable_domain};
use pwm_vault::UrlMatch;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (page, item) = text.split_once('\n').unwrap_or((text, ""));
    let _ = registrable_domain(page);
    let _ = registrable_domain(item);
    // The first line as a package and a digest, the second as a login's link.
    let (package, cert) = page.split_once(' ').unwrap_or((page, ""));
    let _ = app_link(package, cert);
    let _ = match_app(package, &[cert.to_owned()], item);
    let Ok(page) = url::Url::parse(page) else {
        return;
    };
    for mode in [
        UrlMatch::Domain,
        UrlMatch::Host,
        UrlMatch::StartsWith,
        UrlMatch::Regex,
        UrlMatch::Exact,
        UrlMatch::Never,
    ] {
        let _ = match_url(&page, item, &mode);
    }
});
