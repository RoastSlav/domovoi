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

/// How long a request may take; the event stream is exempt.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
    match tokio::time::timeout(REQUEST_TIMEOUT, next.run(req)).await {
        Ok(response) => response,
        Err(_) => (StatusCode::REQUEST_TIMEOUT, "request timed out").into_response(),
    }
}
