//! Structured access logging: one JSON line per completed HTTP request.
//!
//! The field set is a fixed allow-list (see [`AccessEntry`]) — blob
//! contents, request/response bodies, `Authorization` credentials, OpenID
//! tokens, and query strings are never logged. Each response carries an
//! `x-request-id` header matching the logged `request_id` so operators can
//! correlate a client report with its log line.

use axum::extract::{Request, State};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::routes::AppState;

/// One completed request, as logged. Fields are the whole contract —
/// nothing else may appear in an access-log line.
#[derive(Debug, Serialize)]
pub struct AccessEntry {
    /// RFC 3339 timestamp of request completion.
    pub ts: String,
    pub request_id: String,
    pub method: String,
    /// Path only — never the query string (it can carry secrets upstream).
    pub path: String,
    pub status: u16,
    pub duration_ms: u64,
    /// Matrix user id when the request carried a valid session, else null.
    pub user_id: Option<String>,
}

/// Where access entries go: stderr as JSON lines in production, a captured
/// buffer in tests.
pub trait LogSink: Send + Sync {
    fn log(&self, entry: &AccessEntry);
}

/// Renders one access entry as a single JSON line (no trailing newline).
pub fn render(entry: &AccessEntry) -> String {
    serde_json::to_string(entry)
        // Infallible: plain string/number fields, no map keys.
        .expect("serializing an AccessEntry cannot fail")
}

/// Production sink: one JSON line per request on stderr.
pub struct StderrLogSink;

impl LogSink for StderrLogSink {
    fn log(&self, entry: &AccessEntry) {
        let mut line = render(entry);
        line.push('\n');
        eprint!("{line}");
    }
}

/// Test sink: collects rendered lines for assertions.
#[derive(Default, Clone)]
pub struct CapturingSink {
    lines: Arc<Mutex<Vec<String>>>,
}

impl CapturingSink {
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("capturing sink poisoned").clone()
    }
}

impl LogSink for CapturingSink {
    fn log(&self, entry: &AccessEntry) {
        self.lines
            .lock()
            .expect("capturing sink poisoned")
            .push(render(entry));
    }
}

/// Axum middleware: mints a request id, resolves the caller, times the
/// call, and emits one [`AccessEntry`] after the handler responds. The
/// request id is echoed back as `x-request-id`.
pub async fn access_log(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    // Resolved here for the log; the auth extractor resolves again in the
    // handler — a cheap map lookup next to blob I/O.
    let user_id = bearer_user(&req, &state);

    let start = std::time::Instant::now();
    let mut res = next.run(req).await;

    let entry = AccessEntry {
        ts: jiff::Zoned::now().to_string(),
        request_id: request_id.clone(),
        method,
        path,
        status: res.status().as_u16(),
        duration_ms: start.elapsed().as_millis() as u64,
        user_id,
    };
    state.log_sink.log(&entry);

    res.headers_mut().insert(
        axum::http::header::HeaderName::from_static("x-request-id"),
        // Infallible: a UUID string is always a valid header value.
        HeaderValue::from_str(&request_id).expect("uuid is valid header bytes"),
    );
    res
}

/// The user id behind a request's `Authorization: Bearer <session>` header,
/// if the session is valid.
fn bearer_user(req: &Request, state: &AppState) -> Option<String> {
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = header.strip_prefix("Bearer ")?;
    state.sessions.resolve(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AccessEntry {
        AccessEntry {
            ts: "2026-10-23T10:15:00+02:00[Europe/Paris]".into(),
            request_id: "6f9619ff-8b86-d011-b42d-00cf4fc964ff".into(),
            method: "POST".into(),
            path: "/v1/blobs".into(),
            status: 201,
            duration_ms: 3,
            user_id: Some("@lisa:example.org".into()),
        }
    }

    #[test]
    fn render_yields_single_json_line() {
        let line = render(&sample());
        assert!(!line.contains('\n'), "one line, no embedded newlines");
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["path"], "/v1/blobs");
        assert_eq!(parsed["status"], 201);
        assert_eq!(parsed["user_id"], "@lisa:example.org");
    }
}
