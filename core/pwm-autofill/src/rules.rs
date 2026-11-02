//! The signed rules file (D33 §7): per-site field overrides, "never fill"
//! sites and multi-step hints, shipped apart from app releases.
//!
//! Wire format: `v:u8 (= 1) || key_id:u8 || signature:64 || json`, where the
//! Ed25519 signature (label `pwm/autofill-rules`) covers `key_id || json`.

use std::collections::HashMap;

use pwm_crypto::{SignatureContext, verify_signature};
use serde::Deserialize;
use url::Url;

use crate::matching::site_of;
use crate::{Class, Field, registrable_domain};

const WIRE_VERSION: u8 = 1;
const JSON_VERSION: u8 = 1;
const HEADER: usize = 1 + 1 + 64;
const MAX_JSON: usize = 1 << 20;
const MAX_SITES: usize = 50_000;
const MAX_LIFETIME_MS: i64 = 90 * 24 * 60 * 60 * 1000;

/// The release keys that may sign rules files, as `(key_id, public key)`:
/// the current key and the next, so the key can be rotated. Empty until the
/// release owner creates the keys offline; until then no downloaded file is
/// accepted and the built-in heuristics alone apply.
pub const RULES_KEYS: &[(u8, [u8; 32])] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RulesError {
    #[error("the rules file is malformed")]
    Malformed,
    #[error("the rules file format is not supported")]
    UnsupportedVersion,
    #[error("the rules file is signed by an unknown key")]
    UnknownKey,
    #[error("the rules file signature is invalid")]
    BadSignature,
    #[error("the rules file is older than one already accepted")]
    Rollback,
    #[error("the rules file has expired")]
    Expired,
    #[error("the rules file is too large")]
    TooLarge,
}

/// Which attribute a field rule looks at. Never CSS, so every platform can
/// apply it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attr {
    Id,
    Name,
    Autocomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Test {
    Equals(String),
    Contains(String),
}

/// "A field whose `attr` equals (or contains) this text is `class`."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRule {
    attr: Attr,
    test: Test,
    pub class: Class,
}

impl FieldRule {
    pub fn by_id_equals(id: &str, class: Class) -> Self {
        Self {
            attr: Attr::Id,
            test: Test::Equals(id.to_lowercase()),
            class,
        }
    }

    pub fn by_name_contains(text: &str, class: Class) -> Self {
        Self {
            attr: Attr::Name,
            test: Test::Contains(text.to_lowercase()),
            class,
        }
    }

    pub(crate) fn matches(&self, field: &Field) -> bool {
        let value = match self.attr {
            Attr::Id => &field.id,
            Attr::Name => &field.name,
            Attr::Autocomplete => &field.autocomplete,
        }
        .to_lowercase();
        match &self.test {
            Test::Equals(text) => value == *text,
            Test::Contains(text) => value.contains(text.as_str()),
        }
    }
}

/// The rules for one site (a registrable domain, optionally some paths).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiteRules {
    pub domain: String,
    /// Path prefixes this entry applies to; empty means the whole site.
    pub paths: Vec<String>,
    pub never_fill: bool,
    /// The site logs in over several pages.
    pub multi_step: bool,
    pub fields: Vec<FieldRule>,
}

/// An accepted rules file.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    version: u64,
    sites: HashMap<String, Vec<SiteRules>>,
}

impl Rules {
    /// No rules: the built-in heuristics alone.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// The rules for the page at `url`, if any.
    pub fn site_for(&self, url: &Url) -> Option<&SiteRules> {
        let site = site_of(url)?;
        self.sites.get(&site)?.iter().find(|s| {
            s.paths.is_empty() || s.paths.iter().any(|p| url.path().starts_with(p.as_str()))
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileJson {
    v: u8,
    version: u64,
    issued_at: i64,
    expires_at: i64,
    sites: Vec<SiteJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteJson {
    domain: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    never_fill: bool,
    #[serde(default)]
    multi_step: bool,
    #[serde(default)]
    fields: Vec<FieldJson>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldJson {
    attr: Attr,
    #[serde(default)]
    equals: Option<String>,
    #[serde(default)]
    contains: Option<String>,
    class: String,
}

fn site(json: SiteJson) -> Result<SiteRules, RulesError> {
    let domain = json.domain.to_lowercase();
    // Entries name a registrable domain exactly ("example.com").
    if registrable_domain(&domain).as_deref() != Some(domain.as_str()) {
        return Err(RulesError::Malformed);
    }
    if json.paths.iter().any(|p| !p.starts_with('/')) {
        return Err(RulesError::Malformed);
    }
    let fields = json
        .fields
        .into_iter()
        .map(|f| {
            let test = match (f.equals, f.contains) {
                (Some(text), None) => Test::Equals(text.to_lowercase()),
                (None, Some(text)) => Test::Contains(text.to_lowercase()),
                _ => return Err(RulesError::Malformed),
            };
            let class = Class::from_token(&f.class).ok_or(RulesError::Malformed)?;
            Ok(FieldRule {
                attr: f.attr,
                test,
                class,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(SiteRules {
        domain,
        paths: json.paths,
        never_fill: json.never_fill,
        multi_step: json.multi_step,
        fields,
    })
}

/// Checks and parses a rules file. `keys` are the trusted release keys
/// ([`RULES_KEYS`] in the apps), `min_version` the highest version accepted
/// before (stored by the platform), so an older file is refused.
pub fn verify_rules(
    bytes: &[u8],
    keys: &[(u8, [u8; 32])],
    now_ms: i64,
    min_version: u64,
) -> Result<Rules, RulesError> {
    if bytes.len() < HEADER {
        return Err(RulesError::Malformed);
    }
    if bytes.len() - HEADER > MAX_JSON {
        return Err(RulesError::TooLarge);
    }
    if bytes[0] != WIRE_VERSION {
        return Err(RulesError::UnsupportedVersion);
    }
    let key_id = bytes[1];
    let (_, key) = keys
        .iter()
        .find(|(id, _)| *id == key_id)
        .ok_or(RulesError::UnknownKey)?;
    let signature: [u8; 64] = bytes[2..HEADER]
        .try_into()
        .map_err(|_| RulesError::Malformed)?;
    let json = &bytes[HEADER..];
    let message = [&[key_id][..], json].concat();
    verify_signature(key, SignatureContext::AutofillRules, &message, &signature)
        .map_err(|_| RulesError::BadSignature)?;

    let file: FileJson = serde_json::from_slice(json).map_err(|_| RulesError::Malformed)?;
    if file.v != JSON_VERSION {
        return Err(RulesError::UnsupportedVersion);
    }
    if file.sites.len() > MAX_SITES {
        return Err(RulesError::TooLarge);
    }
    if file.expires_at <= file.issued_at || file.expires_at - file.issued_at > MAX_LIFETIME_MS {
        return Err(RulesError::Malformed);
    }
    if file.version < min_version {
        return Err(RulesError::Rollback);
    }
    if now_ms >= file.expires_at {
        return Err(RulesError::Expired);
    }
    let mut sites: HashMap<String, Vec<SiteRules>> = HashMap::new();
    for json in file.sites {
        let entry = site(json)?;
        sites.entry(entry.domain.clone()).or_default().push(entry);
    }
    Ok(Rules {
        version: file.version,
        sites,
    })
}
