//! API errors as JSON `{ error, message }` with a matching status code.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

use crate::auth::AuthError;

/// The body of every error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ErrorBody {
    /// A stable machine-readable code, for example `not_found`.
    pub error: String,
    /// One line for people.
    pub message: String,
}

/// Why a request failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    /// No valid session.
    #[error("log in first")]
    Unauthorized,
    /// Login failed; deliberately says nothing about which part was wrong.
    #[error("wrong username or password")]
    BadCredentials,
    /// Not allowed.
    #[error("{0}")]
    Forbidden(String),
    /// Nothing with that id or name.
    #[error("not found")]
    NotFound,
    /// The request is malformed or invalid.
    #[error("{0}")]
    BadRequest(String),
    /// The request conflicts with the current state.
    #[error("{0}")]
    Conflict(String),
    /// Too many attempts; wait this many seconds.
    #[error("too many attempts; try again in {0} s")]
    TooMany(i64),
    /// A part the request needs is not configured or not running.
    #[error("{0}")]
    Unavailable(String),
    /// Something failed inside the hub; details are logged, not returned.
    #[error("internal error")]
    Internal,
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::BadCredentials => (StatusCode::UNAUTHORIZED, "bad_credentials"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::TooMany(_) => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }

    /// An internal failure: logged with `context`, answered without details.
    pub fn internal(context: &str, err: impl std::fmt::Display) -> Self {
        tracing::error!(%err, "{context}");
        Self::Internal
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        let body = ErrorBody {
            error: code.to_owned(),
            message: self.to_string(),
        };
        let mut response = (status, Json(body)).into_response();
        if let Self::TooMany(secs) = self
            && let Ok(value) = HeaderValue::from_str(&secs.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::InvalidCredentials => Self::BadCredentials,
            AuthError::LockedOut { retry_after_secs } => Self::TooMany(retry_after_secs),
            AuthError::SetupClosed => Self::Conflict(err.to_string()),
            AuthError::BadSetupToken => Self::Forbidden(err.to_string()),
            AuthError::WeakPassword | AuthError::InvalidUsername => {
                Self::BadRequest(err.to_string())
            }
            AuthError::Store(e) => Self::internal("auth store failed", e),
            AuthError::Hash => Self::internal("password hashing failed", "argon2"),
        }
    }
}

impl From<domovoi_core::ports::StoreError> for ApiError {
    fn from(err: domovoi_core::ports::StoreError) -> Self {
        match err {
            domovoi_core::ports::StoreError::NotFound => Self::NotFound,
            domovoi_core::ports::StoreError::Conflict(m) => Self::Conflict(m),
            other => Self::internal("store failed", other),
        }
    }
}
