//! Autofill through the client (D33 §1): which logins a page may get, what
//! each field is, the signed rules file and multi-step state. The engine is
//! `pwm-autofill`; this only feeds it the unlocked vault.

use pwm_autofill::{
    Decision, Field, FrameRelation, Match, MultiStep, NextStep, Problem, RULES_KEYS, RefuseReason,
    ReportFrame, ReportInput, Rules, StepDone, WarnReason, build_report, classify, rank,
    verify_rules,
};
use pwm_store::Store;
use pwm_vault::ItemKind;
use serde::Serialize;

use crate::{Client, ClientError};

/// A login the page may get, best first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutofillMatch {
    pub vault_id: String,
    pub item_id: String,
    pub title: String,
    pub username: String,
    /// `exact`, `starts_with`, `host`, `domain` or `regex`.
    pub matched: &'static str,
    /// `fill`, `warn_insecure`, `warn_regex`, `refuse_cross_site`,
    /// `refuse_blocked` or `confirm`. Every fill still waits for the user.
    pub decision: &'static str,
}

fn matched(m: Match) -> &'static str {
    match m {
        Match::Exact => "exact",
        Match::StartsWith => "starts_with",
        Match::Host => "host",
        Match::Domain => "domain",
        Match::Regex => "regex",
    }
}

fn decision(d: Decision) -> &'static str {
    match d {
        Decision::Fill => "fill",
        Decision::Warn(WarnReason::InsecurePage) => "warn_insecure",
        Decision::Warn(WarnReason::RegexOnly) => "warn_regex",
        Decision::Refuse(RefuseReason::CrossSiteFrame) => "refuse_cross_site",
        Decision::Refuse(RefuseReason::SiteBlocked) => "refuse_blocked",
        Decision::Confirm => "confirm",
    }
}

#[derive(Serialize)]
struct ClassJson {
    class: &'static str,
    confidence: f32,
}

/// Autofill state the client keeps beside the vault.
#[derive(Default)]
pub(crate) struct AutofillState {
    pub(crate) rules: Rules,
    steps: MultiStep,
}

impl<S: Store> Client<S> {
    /// The logins this page (or the frame holding the fields) may get, best
    /// first. Trashed items are left out.
    pub fn autofill_matches(
        &self,
        page_url: &str,
        frame_url: Option<&str>,
    ) -> Result<Vec<AutofillMatch>, ClientError> {
        let engine = self.engine_ref()?;
        let mut logins = Vec::new();
        for vault_id in self.vault_ids()? {
            let id = crate::client::parse_id(&vault_id)?;
            for (item_id, item) in engine.items(&id)? {
                if item.is_trashed() {
                    continue;
                }
                if let ItemKind::Login(login) = &item.kind {
                    logins.push((
                        (
                            vault_id.clone(),
                            item_id,
                            item.title.clone(),
                            login.username.clone(),
                        ),
                        login.urls.clone(),
                        item.updated_at,
                    ));
                }
            }
        }
        let items: Vec<_> = logins
            .iter()
            .map(|(key, urls, used)| (key.clone(), urls.as_slice(), *used))
            .collect();
        let ranked = rank(page_url, frame_url, &items, &self.autofill.rules)
            .map_err(|_| ClientError::InvalidUrl)?;
        Ok(ranked
            .into_iter()
            .map(|c| {
                let (vault_id, item_id, title, username) = c.key;
                AutofillMatch {
                    vault_id,
                    item_id: item_id.to_string(),
                    title,
                    username,
                    matched: matched(c.matched),
                    decision: decision(c.decision),
                }
            })
            .collect())
    }

    /// Classifies the fields described in `fields_json` (an array of field
    /// descriptions, never values) on the page at `page_url`. Returns a JSON
    /// array of `{class, confidence}`, one per field.
    pub fn classify_fields(
        &self,
        fields_json: &str,
        page_url: &str,
    ) -> Result<String, ClientError> {
        let fields: Vec<Field> =
            serde_json::from_str(fields_json).map_err(|_| ClientError::InvalidItem)?;
        let url = url::Url::parse(page_url).map_err(|_| ClientError::InvalidUrl)?;
        let site = self.autofill.rules.site_for(&url);
        let classes: Vec<ClassJson> = classify(&fields, site)
            .into_iter()
            .map(|c| ClassJson {
                class: c.class.as_str(),
                confidence: c.confidence,
            })
            .collect();
        serde_json::to_string(&classes).map_err(|_| ClientError::Internal)
    }

    /// Accepts a downloaded rules file if it is signed by a release key, not
    /// older than `min_version` and not expired. Returns its version, which
    /// the platform stores as the next `min_version`. A refused file leaves
    /// the current rules in place.
    pub fn set_autofill_rules(
        &mut self,
        bytes: &[u8],
        now_ms: i64,
        min_version: u64,
    ) -> Result<u64, ClientError> {
        let rules = verify_rules(bytes, RULES_KEYS, now_ms, min_version)
            .map_err(|_| ClientError::InvalidRules)?;
        let version = rules.version();
        self.autofill.rules = rules;
        Ok(version)
    }

    /// Notes that `item_id` filled a step (`username` or `password`) on this
    /// tab, for multi-step logins.
    pub fn autofill_record_step(
        &mut self,
        tab: &str,
        page_url: &str,
        item_id: &str,
        step: &str,
        now_ms: i64,
    ) -> Result<(), ClientError> {
        let done = match step {
            "username" => StepDone::Username,
            "password" => StepDone::Password,
            _ => return Err(ClientError::InvalidItem),
        };
        self.autofill
            .steps
            .record(tab, page_url, item_id, done, now_ms)
            .map_err(|_| ClientError::InvalidUrl)
    }

    /// The item and step (`password` or `totp`) to offer next on this tab,
    /// if a login is in progress on this site.
    pub fn autofill_next_step(
        &mut self,
        tab: &str,
        page_url: &str,
        now_ms: i64,
    ) -> Option<(String, &'static str)> {
        self.autofill
            .steps
            .next(tab, page_url, now_ms)
            .map(|(item, next)| {
                let next = match next {
                    NextStep::Password => "password",
                    NextStep::Totp => "totp",
                };
                (item, next)
            })
    }

    /// The tab closed.
    pub fn autofill_forget_tab(&mut self, tab: &str) {
        self.autofill.steps.forget(tab);
    }

    /// An "Autofill didn't work here" report (D38) for the user to look at
    /// and maybe send. `frames_json` is a JSON array of `{relation: "top" |
    /// "same_site" | "other_site", fields: [field descriptions], fill:
    /// [indexes]}`; the core classifies the fields itself. Whether a saved
    /// login matched is included only while unlocked.
    pub fn autofill_report(
        &self,
        page_url: &str,
        problem: &str,
        frames_json: &str,
        extension_version: &str,
        browser: &str,
    ) -> Result<String, ClientError> {
        #[derive(serde::Deserialize)]
        struct FrameJson {
            relation: String,
            #[serde(default)]
            fields: Vec<Field>,
            #[serde(default)]
            fill: Vec<usize>,
        }
        let problem = Problem::parse(problem).ok_or(ClientError::InvalidItem)?;
        let raw: Vec<FrameJson> =
            serde_json::from_str(frames_json).map_err(|_| ClientError::InvalidItem)?;
        let url = url::Url::parse(page_url).map_err(|_| ClientError::InvalidUrl)?;
        let site = self.autofill.rules.site_for(&url);
        let frames = raw
            .into_iter()
            .map(|f| {
                let relation = match f.relation.as_str() {
                    "top" => FrameRelation::Top,
                    "same_site" => FrameRelation::SameSite,
                    "other_site" => FrameRelation::OtherSite,
                    _ => return Err(ClientError::InvalidItem),
                };
                let classes = classify(&f.fields, site);
                Ok(ReportFrame {
                    relation,
                    fields: f.fields,
                    classes,
                    fill: f.fill,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let matched = if self.is_unlocked() {
            Some(
                self.autofill_matches(page_url, None)?
                    .iter()
                    .any(|m| !m.decision.starts_with("refuse")),
            )
        } else {
            None
        };
        build_report(&ReportInput {
            page_url: page_url.to_owned(),
            problem,
            frames,
            extension_version: extension_version.to_owned(),
            rules_version: self.autofill.rules.version(),
            browser: browser.to_owned(),
            matched,
        })
        .map_err(|_| ClientError::InvalidUrl)
    }
}
