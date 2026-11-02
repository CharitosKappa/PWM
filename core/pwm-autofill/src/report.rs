//! "Autofill didn't work here" reports (decision D38): the page's origin, a
//! form-structure fingerprint and the engine's decisions, never a value.
//! Text from the page (labels, placeholders, names, ids) is cleaned of
//! things that look like personal data before it leaves the device. The
//! user sees the JSON before sending it.

use serde_json::{Value, json};

use crate::{Field, FieldClass};

/// The report format version (`"v"`).
pub const REPORT_VERSION: u32 = 1;
/// Reports over this size are cut down to fit.
pub const MAX_REPORT_BYTES: usize = 32 * 1024;
const MAX_FRAMES: usize = 20;
const MAX_FIELDS: usize = 200;
const MAX_TEXT_CHARS: usize = 40;
const TOKEN_CHARS: usize = 20;

/// What the user says happened (D38 §3). No free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    NothingFilled,
    WrongField,
    NoSaveOffer,
    IconInTheWay,
    Other,
}

impl Problem {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NothingFilled => "nothing_filled",
            Self::WrongField => "wrong_field",
            Self::NoSaveOffer => "no_save_offer",
            Self::IconInTheWay => "icon_in_the_way",
            Self::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::NothingFilled,
            Self::WrongField,
            Self::NoSaveOffer,
            Self::IconInTheWay,
            Self::Other,
        ]
        .into_iter()
        .find(|p| p.as_str() == text)
    }
}

/// How a frame relates to the top page; its URL is never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRelation {
    Top,
    SameSite,
    OtherSite,
}

impl FrameRelation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::SameSite => "same_site",
            Self::OtherSite => "other_site",
        }
    }
}

pub struct ReportFrame {
    pub relation: FrameRelation,
    pub fields: Vec<Field>,
    /// The engine's class for each field, in order.
    pub classes: Vec<FieldClass>,
    /// Indexes of the fields a fill would use.
    pub fill: Vec<usize>,
}

pub struct ReportInput {
    pub page_url: String,
    pub problem: Problem,
    pub frames: Vec<ReportFrame>,
    pub extension_version: String,
    pub rules_version: u64,
    /// Name and major version, such as "Chrome 140".
    pub browser: String,
    /// Whether a saved login matched the site; `None` while locked.
    pub matched: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReportError {
    #[error("only web pages can be reported")]
    InvalidUrl,
}

fn trim_word(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '@')
}

fn is_email(word: &str) -> bool {
    let word = trim_word(word);
    match word.split_once('@') {
        Some((local, domain)) => !local.is_empty() && domain.contains('.'),
        None => false,
    }
}

/// 20+ characters of letters, digits, `-` or `_`, with at least one digit:
/// session ids, keys and the like.
fn is_token(word: &str) -> bool {
    word.chars().count() >= TOKEN_CHARS
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && word.chars().any(|c| c.is_ascii_digit())
}

/// Runs of 3 or more digits become `#`.
fn mask_digits(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.chars().count() >= 3 {
            out.push('#');
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in word.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Cleans page text for a report (D38 §4): lower case, whitespace
/// collapsed, emails, digit runs and tokens replaced, at most 40 characters.
pub fn clean_text(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            let lower = w.to_lowercase();
            if is_email(&lower) {
                let core = trim_word(&lower);
                lower.replacen(core, "<email>", 1)
            } else if is_token(trim_word(&lower)) {
                let core = trim_word(&lower);
                lower.replacen(core, "<token>", 1)
            } else {
                mask_digits(&lower)
            }
        })
        .collect();
    words.join(" ").chars().take(MAX_TEXT_CHARS).collect()
}

/// The page's origin, or an error for anything but a web page.
fn origin(page_url: &str) -> Result<String, ReportError> {
    let url = url::Url::parse(page_url).map_err(|_| ReportError::InvalidUrl)?;
    match url.scheme() {
        "https" | "http" if url.host_str().is_some() => Ok(url.origin().ascii_serialization()),
        _ => Err(ReportError::InvalidUrl),
    }
}

fn field_json(field: &Field, class: Option<&FieldClass>, fill: bool) -> Value {
    json!({
        "form": field.form,
        "tag": clean_text(&field.tag),
        "type": clean_text(&field.r#type),
        "autocomplete": clean_text(&field.autocomplete),
        "inputmode": clean_text(&field.inputmode),
        "max_length": field.max_length,
        "visible": field.visible,
        "name": clean_text(&field.name),
        "id": clean_text(&field.id),
        "label": clean_text(&field.label),
        "placeholder": clean_text(&field.placeholder),
        "aria_label": clean_text(&field.aria_label),
        "class": class.map(|c| c.class.as_str()),
        "confidence": class.map(|c| (c.confidence * 100.0).round() / 100.0),
        "fill": fill,
    })
}

/// The report JSON (D38 §3), within the size and count limits.
pub fn build_report(input: &ReportInput) -> Result<String, ReportError> {
    let site = origin(&input.page_url)?;
    let mut budget = MAX_FIELDS;
    let mut frames = Vec::new();
    for frame in input.frames.iter().take(MAX_FRAMES) {
        if budget == 0 {
            break;
        }
        let fields: Vec<Value> = frame
            .fields
            .iter()
            .take(budget)
            .enumerate()
            .map(|(i, f)| field_json(f, frame.classes.get(i), frame.fill.contains(&i)))
            .collect();
        budget -= fields.len();
        frames.push(json!({ "relation": frame.relation.as_str(), "fields": fields }));
    }
    let mut report = json!({
        "v": REPORT_VERSION,
        "site": site,
        "problem": input.problem.as_str(),
        "matched": input.matched,
        "software": {
            // From the extension itself, not the page: kept as given, bounded.
            "extension": input.extension_version.chars().take(MAX_TEXT_CHARS).collect::<String>(),
            "core": env!("CARGO_PKG_VERSION"),
            "rules": input.rules_version,
            "browser": input.browser.chars().take(MAX_TEXT_CHARS).collect::<String>(),
        },
        "frames": frames,
    });
    // Too big: leave out frames from the end until it fits.
    let mut text = report.to_string();
    while text.len() > MAX_REPORT_BYTES {
        let Some(list) = report["frames"].as_array_mut() else {
            break;
        };
        if list.pop().is_none() {
            break;
        }
        text = report.to_string();
    }
    Ok(text)
}
