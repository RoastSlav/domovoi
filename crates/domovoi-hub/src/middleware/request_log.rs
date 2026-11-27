//! A request id on every response and one log line per request.
//!
//! Every request runs inside a `request` span that carries `request_id`, so each line a
//! handler logs while serving it (an internal error, a refused login) names the same id as
//! the `x-request-id` response header. A person who reports "it said internal error" can
//! hand over the header and the log line is found by that id.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument;

/// The response header that carries the request id.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// The signed-in user of a request, filled in by the session extractor so the request log
/// line can name who made the call.
#[derive(Debug, Clone, Default)]
pub struct RequestUser(Arc<Mutex<Option<String>>>);

impl RequestUser {
    /// Records `name` as the user of this request.
    pub fn set(&self, name: &str) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(name.to_owned());
        }
    }

    fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|slot| slot.clone())
    }
}

/// Names `username` as the user of the request that owns `extensions`, for its log lines.
pub fn note_user(extensions: &axum::http::Extensions, username: &str) {
    if let Some(user) = extensions.get::<RequestUser>() {
        user.set(username);
    }
    tracing::Span::current().record("user", username);
}

/// How loudly the line for a response with `status` is logged. A success is a debug line,
/// so the default log is not one line per page poll. A refusal is information. A 502 or 503
/// is an expected state (a chat platform that did not take a message, no model configured),
/// not a fault of the hub, so it is information too. Only a 5xx the hub did not expect is
/// an error.
#[must_use]
pub fn level_for(status: StatusCode) -> tracing::Level {
    match status.as_u16() {
        500 | 501 | 505..=599 => tracing::Level::ERROR,
        504 | 429 | 403 => tracing::Level::WARN,
        400..=499 | 502 | 503 => tracing::Level::INFO,
        _ => tracing::Level::DEBUG,
    }
}

/// Gives the request an id, runs it inside a span carrying the id, sets `x-request-id` on
/// the response and logs one line when it ends.
pub async fn log_requests(mut req: Request, next: Next) -> Response {
    let id = uuid::Uuid::now_v7().simple().to_string();
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let user = RequestUser::default();
    req.extensions_mut().insert(user.clone());
    // The method and path ride on the span, so every line logged while serving the request
    // (an internal error, a refused password) names the route it belongs to.
    let span = tracing::info_span!(
        "request",
        request_id = %id,
        method = %method,
        path = %path,
        user = tracing::field::Empty
    );
    let started = Instant::now();
    let mut response = next.run(req).instrument(span.clone()).await;
    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    let status = response.status();
    let who = user.get().unwrap_or_else(|| "-".to_owned());
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let _entered = span.enter();
    macro_rules! line {
        ($level:expr) => {
            tracing::event!(
                $level,
                status = status.as_u16(),
                user = %who,
                duration_ms = ms,
                "request"
            )
        };
    }
    match level_for(status) {
        tracing::Level::ERROR => line!(tracing::Level::ERROR),
        tracing::Level::WARN => line!(tracing::Level::WARN),
        tracing::Level::INFO => line!(tracing::Level::INFO),
        _ => line!(tracing::Level::DEBUG),
    }
    response
}

#[cfg(test)]
mod tests {
    use super::level_for;
    use axum::http::StatusCode;

    #[test]
    fn expected_failures_are_not_errors() {
        assert_eq!(level_for(StatusCode::BAD_GATEWAY), tracing::Level::INFO);
        assert_eq!(
            level_for(StatusCode::SERVICE_UNAVAILABLE),
            tracing::Level::INFO
        );
        assert_eq!(level_for(StatusCode::BAD_REQUEST), tracing::Level::INFO);
        assert_eq!(
            level_for(StatusCode::INTERNAL_SERVER_ERROR),
            tracing::Level::ERROR
        );
        assert_eq!(level_for(StatusCode::OK), tracing::Level::DEBUG);
    }
}
