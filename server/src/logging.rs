//! Logs (D29): JSON lines or text, one line per request, and a scrubber
//! that every line passes through before it is written.

use std::borrow::Cow;
use std::io::{self, Write};
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use tracing::Subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

const REDACTED: &str = "[REDACTED]";
/// Runs of base64 characters at least this long are taken for secrets or
/// ciphertext. Hyphenated UUIDs (36) stay readable.
const LONG_RUN: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Text,
}

impl LogFormat {
    /// `PWM_LOG_FORMAT` (`json` or `text`); JSON unless writing to a terminal.
    pub fn from_env(is_terminal: bool) -> Self {
        match std::env::var("PWM_LOG_FORMAT").as_deref() {
            Ok("json") => Self::Json,
            Ok("text") => Self::Text,
            _ if is_terminal => Self::Text,
            _ => Self::Json,
        }
    }
}

/// The server's log pipeline: `filter` (an `EnvFilter` directive such as
/// `info,sqlx=warn`), `format`, and the scrubber in front of `writer`.
pub fn log_subscriber<W>(
    format: LogFormat,
    filter: &str,
    writer: W,
) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Scrubbing(writer));
    match format {
        LogFormat::Json => Box::new(builder.json().flatten_event(false).finish()),
        LogFormat::Text => Box::new(builder.with_ansi(false).finish()),
    }
}

/// Logs panics by location only: a panic message can quote values.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().map_or_else(
            || "unknown".to_owned(),
            |l| format!("{}:{}", l.file(), l.line()),
        );
        tracing::error!(%location, "panic");
    }));
}

/// One log line per request: a random request id (also sent back as
/// `x-request-id`), method, route template, status and duration. Never the
/// real path, query, headers, IP or ids (D29).
pub(crate) async fn request_log(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let request_id = pwm_crypto::generate_id().map_or_else(
        |_| "0".repeat(32),
        |id| id.iter().map(|b| format!("{b:02x}")).collect(),
    );
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    let mut response = next.run(request).await;
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        target: "pwm_server::request",
        request_id = %request_id,
        method = %method,
        route = %route,
        status = response.status().as_u16(),
        duration_ms,
        "request"
    );
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// Wraps a writer so that every log line is scrubbed before it is written.
pub struct Scrubbing<M>(pub M);

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for Scrubbing<M> {
    type Writer = ScrubWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        ScrubWriter {
            inner: self.0.make_writer(),
            buf: Vec::new(),
        }
    }
}

/// Collects one event, then writes it scrubbed when flushed or dropped, so
/// a secret split across two writes is still caught.
pub struct ScrubWriter<W: Write> {
    inner: W,
    buf: Vec<u8>,
}

impl<W: Write> ScrubWriter<W> {
    fn emit(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.buf);
        let clean = scrub(&text);
        let result = self.inner.write_all(clean.as_bytes());
        self.buf.clear();
        result
    }
}

impl<W: Write> Write for ScrubWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.emit()?;
        self.inner.flush()
    }
}

impl<W: Write> Drop for ScrubWriter<W> {
    fn drop(&mut self) {
        let _ = self.emit();
    }
}

/// Base64 and base64url characters, without the `=` padding (which would
/// also swallow the `key=` before a value).
fn is_run_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b'/')
}

fn is_local_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_domain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')
}

/// Replaces email addresses and runs of 40 or more base64 characters
/// (tokens, keys, ciphertext, OPAQUE messages) with `[REDACTED]`. Every
/// pattern is ASCII, so cuts never split a UTF-8 character.
pub fn scrub(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    // Email addresses: something@domain.tld.
    for (at, _) in text.match_indices('@') {
        let start = bytes[..at]
            .iter()
            .rposition(|b| !is_local_byte(*b))
            .map_or(0, |i| i + 1);
        let end = bytes[at + 1..]
            .iter()
            .position(|b| !is_domain_byte(*b))
            .map_or(bytes.len(), |i| at + 1 + i);
        let domain = text[at + 1..end].trim_end_matches('.');
        if start < at && domain.contains('.') && !domain.starts_with('.') {
            spans.push((start, at + 1 + domain.len()));
        }
    }
    // Long base64-looking runs.
    let mut i = 0;
    while i < bytes.len() {
        if is_run_byte(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_run_byte(bytes[i]) {
                i += 1;
            }
            if i - start >= LONG_RUN {
                // Up to two padding characters belong to the run.
                let padding = bytes[i..]
                    .iter()
                    .take(2)
                    .take_while(|b| **b == b'=')
                    .count();
                spans.push((start, i + padding));
            }
        } else {
            i += 1;
        }
    }
    if spans.is_empty() {
        return Cow::Borrowed(text);
    }
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end) in spans {
        if end <= last {
            continue;
        }
        let start = start.max(last);
        out.push_str(&text[last..start]);
        if !out.ends_with(REDACTED) || start != last {
            out.push_str(REDACTED);
        }
        last = end;
    }
    out.push_str(&text[last..]);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_tokens_keys_and_emails() {
        let token = format!("pwma_{}", "A1b2_C3d4-".repeat(5));
        let line = format!("token {token} for Alice.B+tag@mail.example.co.uk, done");
        assert_eq!(scrub(&line), "token [REDACTED] for [REDACTED], done");
        let blob = "ab+/".repeat(12) + "==";
        assert_eq!(scrub(&format!("blob={blob};")), "blob=[REDACTED];");
    }

    #[test]
    fn keeps_ordinary_text() {
        for line in [
            r#"{"level":"INFO","fields":{"route":"/v1/vaults/{vault_id}/items/{item_id}","status":201}}"#,
            "session 0f8fad5b-d9cb-469f-a165-70867728950e ended",
            "listening on 127.0.0.1:8080",
            "no email here: a@b, @example.com, user@localhost",
            "Ünïcode stays",
        ] {
            assert_eq!(scrub(line), line);
        }
    }

    #[test]
    fn overlapping_matches_give_one_marker() {
        let run = "x".repeat(45);
        assert_eq!(scrub(&format!("{run}@example.com")), "[REDACTED]");
        assert_eq!(scrub(&format!("{run} {run}")), "[REDACTED] [REDACTED]");
    }

    #[test]
    fn a_secret_split_across_writes_is_still_caught() {
        let mut out = Vec::new();
        {
            let mut writer = ScrubWriter {
                inner: &mut out,
                buf: Vec::new(),
            };
            let secret = "Z".repeat(50);
            writer.write_all(&secret.as_bytes()[..20]).unwrap();
            writer.write_all(&secret.as_bytes()[20..]).unwrap();
            writer.write_all(b"\n").unwrap();
        }
        assert_eq!(String::from_utf8(out).unwrap(), "[REDACTED]\n");
    }
}
