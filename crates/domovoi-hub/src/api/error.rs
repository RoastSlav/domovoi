//! API errors as JSON `{ error, message }` with a matching status code.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

use jiff::Timestamp;

use crate::auth::AuthError;

/// The body of every error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ErrorBody {
    /// A stable machine-readable code, for example `not_found`.
    pub error: String,
    /// One line for people.
    pub message: String,
    /// For `invalid`: every problem, each with the settings path it is about.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub issues: Vec<IssueDto>,
}

/// One problem with submitted settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct IssueDto {
    /// The dotted settings path, for example `llm.primary.model`; empty for the whole.
    pub path: String,
    /// What is wrong and how to fix it.
    pub message: String,
    /// `error` or `warning`.
    pub severity: String,
}

impl From<&domovoi_config::ConfigIssue> for IssueDto {
    fn from(issue: &domovoi_config::ConfigIssue) -> Self {
        Self {
            path: issue.path.clone(),
            message: issue.message.clone(),
            severity: if issue.is_error() { "error" } else { "warning" }.to_owned(),
        }
    }
}

/// Why a request failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    /// No valid session.
    #[error("log in first")]
    Unauthorized,
    /// No account exists yet; the admin must be created with the setup link first.
    #[error("no account exists yet; open the setup link the hub logged")]
    SetupRequired,
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
    /// Triage, and with it the model chat, is not enabled.
    #[error("triage is not enabled")]
    TriageOff,
    /// The monthly model budget is spent; no model was asked.
    #[error("the monthly model budget is spent")]
    BudgetSpent,
    /// A configured chat platform did not take a message.
    #[error("{0}")]
    Delivery(String),
    /// Submitted settings are not valid; every issue has its path.
    #[error("the settings are not valid")]
    Invalid(Vec<IssueDto>),
    /// A refusal with its own status and stable code, for the package endpoints.
    #[error("{message}")]
    Coded {
        /// The HTTP status.
        status: StatusCode,
        /// The stable machine-readable code, for example `package_conflict`.
        code: &'static str,
        /// One line for people.
        message: String,
    },
    /// Something failed inside the hub; details are logged, not returned.
    #[error("internal error")]
    Internal,
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            Self::Coded { status, code, .. } => (*status, code),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::SetupRequired => (StatusCode::UNAUTHORIZED, "setup_required"),
            Self::BadCredentials => (StatusCode::UNAUTHORIZED, "bad_credentials"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::TooMany(_) => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            Self::TriageOff => (StatusCode::SERVICE_UNAVAILABLE, "triage_off"),
            Self::BudgetSpent => (StatusCode::SERVICE_UNAVAILABLE, "budget_spent"),
            Self::Delivery(_) => (StatusCode::BAD_GATEWAY, "delivery_failed"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }

    /// An internal failure: logged with `context`, answered without details.
    pub fn internal(context: &str, err: impl std::fmt::Display) -> Self {
        tracing::error!(%err, "{context}");
        Self::Internal
    }
}

/// The 429 for a busy model hour: `retry_at` is when the hour has room again, if
/// the engine knows; the `Retry-After` header tells the browser when to come back.
pub fn busy(now: Timestamp, retry_at: Option<Timestamp>) -> ApiError {
    let seconds = retry_at.map_or(30, |at| at.duration_since(now).as_secs().max(1));
    ApiError::TooMany(seconds)
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        let body = ErrorBody {
            error: code.to_owned(),
            message: self.to_string(),
            issues: match &self {
                Self::Invalid(issues) => issues.clone(),
                _ => Vec::new(),
            },
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

/// Turns the plain-text rejection axum gives a request body it cannot read (bad or missing
/// JSON, wrong content type) into the API's own `{ error, message }` body, so no route
/// answers with a raw serde message and no error code.
pub(super) async fn json_rejections(response: Response) -> Response {
    let status = response.status();
    let plain = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/plain"));
    if !plain
        || !matches!(
            status,
            StatusCode::BAD_REQUEST
                | StatusCode::UNPROCESSABLE_ENTITY
                | StatusCode::UNSUPPORTED_MEDIA_TYPE
        )
    {
        return response;
    }
    let (_, body) = response.into_parts();
    let text = match axum::body::to_bytes(body, 4096).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).trim().to_owned(),
        Err(_) => String::new(),
    };
    let message = if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
        "send the body as JSON with the header Content-Type: application/json".to_owned()
    } else if text.contains("EOF while parsing") {
        "the request needs a JSON body and none was sent; send the fields the endpoint lists in the API reference".to_owned()
    } else {
        format!("the request body could not be read: {text}")
    };
    let body = ErrorBody {
        error: if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
            "unsupported_media_type".to_owned()
        } else {
            "bad_request".to_owned()
        },
        message,
        issues: Vec::new(),
    };
    (status, Json(body)).into_response()
}
