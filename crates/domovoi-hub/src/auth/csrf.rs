//! The CSRF guard for `/api/v1` (INV-8): every unsafe method needs the session's
//! `X-CSRF-Token`, and an `Origin` header, when present, must be the hub's own.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{AuthState, session};

/// The header carrying the CSRF token.
pub const HEADER: &str = "x-csrf-token";

/// Paths that create a session and so cannot carry its token yet. They still need a
/// matching `Origin`.
pub const NO_TOKEN_PATHS: [&str; 2] = ["/api/v1/auth/login", "/api/v1/auth/setup"];

fn forbidden(message: &'static str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({ "error": "forbidden", "message": message })),
    )
        .into_response()
}

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// The session cookie value of `req`, if any.
#[must_use]
pub fn session_id(req: &axum::http::HeaderMap) -> Option<&str> {
    req.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(session::from_cookie_header)
}

/// Rejects cross-origin and token-less unsafe requests with 403. Requests without a
/// session pass through; the handler answers them with 401.
pub async fn guard(State(auth): State<Arc<AuthState>>, req: Request, next: Next) -> Response {
    if is_safe(req.method()) {
        return next.run(req).await;
    }
    let headers = req.headers();
    if let Some(origin) = headers.get(header::ORIGIN)
        && origin.to_str().ok() != Some(auth.web().origin().as_str())
    {
        return forbidden("cross-origin requests are not allowed");
    }
    if NO_TOKEN_PATHS.contains(&req.uri().path()) {
        return next.run(req).await;
    }
    let Some(id) = session_id(headers) else {
        return next.run(req).await;
    };
    let token = headers.get(HEADER).and_then(|v| v.to_str().ok());
    match token {
        Some(token) if AuthState::csrf_matches(id, token) => next.run(req).await,
        _ => forbidden("missing or wrong CSRF token"),
    }
}
