//! `POST /v1/reports/autofill` (D38): anonymous "Autofill didn't work here"
//! reports. No token, no account, no device; the IP address is used only
//! for the rate limit and never stored, and bodies are never logged. Each
//! report is checked against the v1 schema so that only structure (never a
//! value) can be stored, and kept 90 days.

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::api::{ApiError, ApiJson, ApiRouter, AppState};

/// The largest report the extension sends (`pwm_autofill::MAX_REPORT_BYTES`).
pub const MAX_REPORT_BYTES: usize = 32 * 1024;
/// Reports older than this are deleted.
pub const REPORT_RETENTION_DAYS: i64 = 90;

const MAX_FRAMES: usize = 20;
const MAX_FIELDS: usize = 200;
/// Cleaned text is at most 40 characters; 4 bytes each at worst.
const MAX_TEXT_BYTES: usize = 160;

const PROBLEMS: &[&str] = &[
    "nothing_filled",
    "wrong_field",
    "no_save_offer",
    "icon_in_the_way",
    "other",
];
const RELATIONS: &[&str] = &["top", "same_site", "other_site"];
const TEXT_FIELDS: &[&str] = &[
    "tag",
    "type",
    "autocomplete",
    "inputmode",
    "name",
    "id",
    "label",
    "placeholder",
    "aria_label",
];

pub fn routes() -> ApiRouter {
    Router::new()
        .route("/v1/reports/autofill", post(receive))
        .layer(DefaultBodyLimit::max(MAX_REPORT_BYTES))
}

fn short_text(value: &Value) -> bool {
    value.as_str().is_some_and(|s| s.len() <= MAX_TEXT_BYTES)
}

fn optional<F: Fn(&Value) -> bool>(value: &Value, check: F) -> bool {
    value.is_null() || check(value)
}

/// `https://host[:port]` or `http://...`: an origin, never a path or query.
fn is_origin(site: &str) -> bool {
    let Some(rest) = site
        .strip_prefix("https://")
        .or_else(|| site.strip_prefix("http://"))
    else {
        return false;
    };
    site.len() <= 300
        && !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

fn only_keys(object: &serde_json::Map<String, Value>, allowed: &[&str]) -> bool {
    object.keys().all(|k| allowed.contains(&k.as_str()))
}

fn field_ok(field: &Value) -> bool {
    let Some(f) = field.as_object() else {
        return false;
    };
    let mut keys: Vec<&str> = TEXT_FIELDS.to_vec();
    keys.extend([
        "form",
        "max_length",
        "visible",
        "class",
        "confidence",
        "fill",
    ]);
    only_keys(f, &keys)
        && TEXT_FIELDS.iter().all(|k| short_text(&f[*k]))
        && optional(&f["form"], Value::is_u64)
        && optional(&f["max_length"], Value::is_u64)
        && f["visible"].is_boolean()
        && f["fill"].is_boolean()
        && optional(&f["class"], short_text)
        && optional(&f["confidence"], Value::is_number)
}

/// The site of a valid v1 report (D38 §3), or why not. Unknown keys are
/// refused, so a value cannot ride along under another name.
pub fn check_report(report: &Value) -> Result<String, ApiError> {
    let bad = ApiError::InvalidRequest;
    let r = report.as_object().ok_or(bad)?;
    if !only_keys(
        r,
        &["v", "site", "problem", "matched", "software", "frames"],
    ) {
        return Err(bad);
    }
    if r.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(bad);
    }
    let site = r.get("site").and_then(Value::as_str).ok_or(bad)?;
    if !is_origin(site) {
        return Err(bad);
    }
    let problem = r.get("problem").and_then(Value::as_str).ok_or(bad)?;
    if !PROBLEMS.contains(&problem) {
        return Err(bad);
    }
    if !optional(&report["matched"], Value::is_boolean) {
        return Err(bad);
    }
    let software = r.get("software").and_then(Value::as_object).ok_or(bad)?;
    let software_ok = only_keys(software, &["extension", "core", "rules", "browser"])
        && ["extension", "core", "browser"]
            .iter()
            .all(|k| short_text(&report["software"][*k]))
        && report["software"]["rules"].is_u64();
    if !software_ok {
        return Err(bad);
    }
    let frames = r.get("frames").and_then(Value::as_array).ok_or(bad)?;
    if frames.len() > MAX_FRAMES {
        return Err(bad);
    }
    let mut fields = 0;
    for frame in frames {
        let f = frame.as_object().ok_or(bad)?;
        let relation = f.get("relation").and_then(Value::as_str).ok_or(bad)?;
        let list = f.get("fields").and_then(Value::as_array).ok_or(bad)?;
        if !only_keys(f, &["relation", "fields"]) || !RELATIONS.contains(&relation) {
            return Err(bad);
        }
        fields += list.len();
        if fields > MAX_FIELDS || !list.iter().all(field_ok) {
            return Err(bad);
        }
    }
    Ok(site.to_owned())
}

async fn receive(
    State(state): State<AppState>,
    ApiJson(report): ApiJson<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let site = check_report(&report)?;
    sqlx::query("INSERT INTO autofill_reports (site, report) VALUES ($1, CAST($2 AS jsonb))")
        .bind(site)
        .bind(report.to_string())
        .execute(&state.pool)
        .await?;
    // Retention (D38 §6): every insert also clears what is too old.
    sqlx::query(
        "DELETE FROM autofill_reports WHERE received_at < now() - make_interval(days => $1::int)",
    )
    .bind(REPORT_RETENTION_DAYS)
    .execute(&state.pool)
    .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({}))))
}

/// The reports received on or after `since` (a date, `YYYY-MM-DD`; all when
/// `None`), oldest first, as JSON lines of `{received_at, site, report}`.
pub async fn export_reports(
    pool: &PgPool,
    since: Option<&str>,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT to_char(received_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), site, report::text \
         FROM autofill_reports \
         WHERE $1::date IS NULL OR received_at >= $1::date \
         ORDER BY received_at, id",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(received_at, site, report)| {
            let report: Value = serde_json::from_str(&report).unwrap_or(Value::Null);
            json!({ "received_at": received_at, "site": site, "report": report }).to_string()
        })
        .collect())
}
