//! What every response gets: security headers, body and time limits, compression and
//! request tracing with secrets redacted.

use std::time::Duration;

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tower_http::set_header::SetResponseHeaderLayer;

/// The largest API request body.
pub const MAX_BODY_BYTES: usize = 1 << 20;

/// How long a request may take; the event stream is exempt and model requests have
/// [`MODEL_REQUEST_TIMEOUT`].
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a request that waits for the model may take. Ask and retry-triage run a whole
/// tool-calling conversation inside their response, and a slow local model needs minutes
/// for it. Each model call has its own idle timeout, so this only bounds a run that keeps
/// producing output.
pub const MODEL_REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The path exempt from the timeout.
pub const EVENTS_PATH: &str = "/api/v1/events";

/// The Content-Security-Policy of every response.
pub const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'";

/// Headers whose values never appear in traces.
pub const SENSITIVE: [HeaderName; 5] = [
    header::COOKIE,
    header::SET_COOKIE,
    header::AUTHORIZATION,
    HeaderName::from_static("x-csrf-token"),
    HeaderName::from_static("x-domovoi-signature"),
];

/// The security headers, each as a layer that overrides whatever a handler set.
#[must_use]
pub fn security_headers() -> [SetResponseHeaderLayer<HeaderValue>; 4] {
    [
        SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        ),
        SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        ),
        SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("interest-cohort=()"),
        ),
    ]
}

/// Ends requests that take longer than [`REQUEST_TIMEOUT`] with 408, except the event
/// stream, which stays open.
pub async fn timeout(req: Request, next: Next) -> Response {
    if req.uri().path() == EVENTS_PATH {
        return next.run(req).await;
    }
    let limit = if is_model_request(req.uri().path()) {
        MODEL_REQUEST_TIMEOUT
    } else {
        REQUEST_TIMEOUT
    };
    match tokio::time::timeout(limit, next.run(req)).await {
        Ok(response) => response,
        Err(_) => (StatusCode::REQUEST_TIMEOUT, "request timed out").into_response(),
    }
}

/// Whether `path` is an incident's `ask` or `triage` endpoint, which wait for the model.
fn is_model_request(path: &str) -> bool {
    path.strip_prefix("/api/v1/incidents/")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(_, action)| action == "ask" || action == "triage")
}

#[cfg(test)]
mod tests {
    use super::is_model_request;

    #[test]
    fn only_ask_and_triage_wait_for_the_model() {
        assert!(is_model_request("/api/v1/incidents/abc/ask"));
        assert!(is_model_request("/api/v1/incidents/abc/triage"));
        assert!(!is_model_request("/api/v1/incidents/abc/ack"));
        assert!(!is_model_request("/api/v1/incidents/abc"));
        assert!(!is_model_request("/api/v1/hosts/abc/ask"));
    }
}
