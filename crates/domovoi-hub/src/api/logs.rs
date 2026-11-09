//! The hub's own recent log lines, for the web UI's Logs page.

use axum::Json;
use axum::extract::{Query, State};

use domovoi_core::ports::{LogLine, LogSource};

use super::dto::{LogLineDto, LogsQuery};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed};

/// Default lines one request asks for.
const DEFAULT_LINES: u16 = 200;

/// Largest number of lines one request may ask for.
const MAX_LINES: u16 = 2000;

/// The least severe level when the request does not say.
const DEFAULT_LEVEL: &str = "info";

/// The hub's recent log lines, oldest first, with secrets removed.
#[utoipa::path(
    get,
    path = "/api/v1/system/logs",
    operation_id = "list_logs",
    tag = "system",
    params(LogsQuery),
    responses(
        (status = 200, description = "The recent log lines", body = Vec<LogLineDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 503, description = "Logs are not captured", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    query: Query<LogsQuery>,
) -> Result<Json<Vec<LogLineDto>>, ApiError> {
    let source = state
        .deps
        .logs
        .clone()
        .ok_or_else(|| ApiError::Unavailable("logs are not captured".to_owned()))?;
    let count = query.lines.unwrap_or(DEFAULT_LINES).clamp(1, MAX_LINES) as usize;
    let min_level = query.level.as_deref().unwrap_or(DEFAULT_LEVEL);
    let picked: Vec<LogLine> = LogSource::tail(&*source, count, min_level);
    Ok(Json(
        picked
            .iter()
            .map(|line| LogLineDto {
                at: line.at.to_string(),
                level: line.level.clone(),
                target: line.target.clone(),
                message: state.deps.redactor.redact(&line.message),
            })
            .collect(),
    ))
}
