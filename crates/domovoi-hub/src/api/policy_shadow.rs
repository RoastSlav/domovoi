//! Recorded shadow decisions and the simulator. Nothing here writes, and nothing here
//! enforces: a recorded decision is data for comparison.

use axum::Json;
use axum::extract::{Path, Query, State};
use domovoi_core::ExecutionId;
use domovoi_core::ports::Page;

use super::dto_policy_query::{ShadowQuery, SimulateRequest};
use super::dto_policy_trace::{ShadowDetailDto, ShadowPageDto, ShadowSummaryRowDto, SimulationDto};
use super::error::{ApiError, ErrorBody};
use super::policy::command_error;
use super::{ApiState, Authed};

/// Replays or re-evaluates recorded Execution steps under a draft, a version or inline rules.
/// It never writes.
#[utoipa::path(
    post, path = "/api/v1/policy/simulate", operation_id = "policy_simulate", tag = "policy",
    request_body = SimulateRequest,
    responses(
        (status = 200, description = "The re-run decisions next to the recorded ones", body = SimulationDto),
        (status = 400, description = "A value or an inline rule does not parse", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such version, or there is no draft", body = ErrorBody),
    )
)]
pub(super) async fn simulate(
    State(state): State<ApiState>,
    _auth: Authed,
    Json(req): Json<SimulateRequest>,
) -> Result<Json<SimulationDto>, ApiError> {
    let result = state
        .services()
        .policy
        .simulate(req.parse()?)
        .await
        .map_err(command_error)?;
    Ok(Json(SimulationDto::from(&result)))
}

/// Recorded shadow decisions, newest first.
#[utoipa::path(
    get, path = "/api/v1/policy/shadow", operation_id = "policy_shadow_list", tag = "policy",
    params(ShadowQuery),
    responses(
        (status = 200, description = "One page", body = ShadowPageDto),
        (status = 400, description = "A filter value does not parse", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<ShadowQuery>,
) -> Result<Json<ShadowPageDto>, ApiError> {
    let filter = query.filter()?;
    let page = Page {
        cursor: query.cursor,
        limit: query.limit.unwrap_or(25).clamp(1, 100),
    };
    let found = state.deps.policy.shadow_records(&filter, &page).await?;
    Ok(Json(ShadowPageDto {
        items: found.items.iter().map(ShadowSummaryRowDto::from).collect(),
        next_cursor: found.next_cursor,
    }))
}

/// One recorded decision with its inputs, gaps and both decisions.
#[utoipa::path(
    get, path = "/api/v1/policy/shadow/{execution_id}", operation_id = "policy_shadow_detail", tag = "policy",
    params(("execution_id" = String, Path, description = "The Execution step")),
    responses(
        (status = 200, description = "The decision", body = ShadowDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No decision recorded for this step yet", body = ErrorBody),
    )
)]
pub(super) async fn detail(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(execution): Path<String>,
) -> Result<Json<ShadowDetailDto>, ApiError> {
    let id: ExecutionId = execution.parse().map_err(|_| ApiError::NotFound)?;
    let row = state
        .deps
        .policy
        .shadow(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(ShadowDetailDto::from(&row)))
}
