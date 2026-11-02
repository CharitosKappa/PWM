//! Autofill URL matching (D33): never panics on any page URL, item URL or
//! pattern, in every mode, and the PSL lookup accepts any host text.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pwm_autofill::{match_url, registrable_domain};
use pwm_vault::UrlMatch;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let (page, item) = text.split_once('\n').unwrap_or((text, ""));
    let _ = registrable_domain(page);
    let _ = registrable_domain(item);
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
