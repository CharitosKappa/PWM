//! The autofill engine (decision D33, `docs/AUTOFILL.md`): site matching on
//! the Public Suffix List, fill decisions, field classification, the signed
//! rules file and multi-step state. Plain Rust with no I/O; every platform
//! describes its page or screen and the core decides.
#![forbid(unsafe_code)]

mod classify;
mod matching;
mod multistep;
mod psl;
mod report;
mod rules;

pub use classify::{Class, Field, FieldClass, classify};
pub use matching::{
    Candidate, Decision, Match, RefuseReason, WarnReason, best_match, match_url, rank,
};
pub use multistep::{MultiStep, NextStep, StepDone};
pub use psl::{PSL_VERSION, registrable_domain};
pub use report::{
    FrameRelation, MAX_REPORT_BYTES, Problem, REPORT_VERSION, ReportError, ReportFrame,
    ReportInput, build_report, clean_text,
};
pub use rules::{Attr, FieldRule, RULES_KEYS, Rules, RulesError, SiteRules, verify_rules};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AutofillError {
    #[error("not a valid page URL")]
    InvalidUrl,
}
