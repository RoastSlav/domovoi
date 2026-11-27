//! Operations: the list, one operation with its trace and executions, one step, and the
//! approval and denial of an Execution step. Approving here is the only way a step becomes
//! authorized (INV-1): the plan hash must be the stored v2 hash, and a destructive skill (or
//! one whose risk is not known) needs its name typed exactly (INV-6).

use axum::Json;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::HeaderMap;
use domovoi_core::ports::OperationFilter;
use domovoi_core::{
    OperationId, OperationKind, OperationSnapshot, OperationStatus, PlanBinding, PlanHash,
    SkillName, StepId, StepKind, build_trace,
};

use super::dto_operations::{
    ApproveStepRequest, ExecutionDto, OperationDetailDto, OperationPage, OperationQuery,
    OperationSummaryDto, OperationTraceDto, StepDetailDto,
};
use super::error::{ApiError, ErrorBody};
use super::operation_errors::command_error;
use super::snapshot::snapshot_seq;
use super::{ApiState, Authed, host_named, page};
use crate::auth::proxy;
use crate::services::{ContainmentApproveInput, StepUp, risk_of_skill};

/// The 503 every operation endpoint but the list answers while orchestration is off.
pub(super) fn off() -> ApiError {
    ApiError::Unavailable("orchestration is not enabled".to_owned())
}

pub(super) fn parse<T: std::str::FromStr>(text: &str) -> Result<T, ApiError> {
    text.parse().map_err(|_| ApiError::NotFound)
}

async fn snapshot(state: &ApiState, id: OperationId) -> Result<OperationSnapshot, ApiError> {
    state
        .deps
        .operation_repo
        .load(id)
        .await?
        .ok_or(ApiError::NotFound)
}

/// The risk of `skill` on the host's current skill list, if it is there.
fn risk_of(
    state: &ApiState,
    host: domovoi_core::HostId,
    skill: Option<&SkillName>,
) -> Option<String> {
    let risk = risk_of_skill(&state.deps.agents.skills(host), skill?)?;
    serde_json::to_value(risk)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
}

/// The execution of Execution step `step`, as a person sees it.
fn execution(
    state: &ApiState,
    snapshot: &OperationSnapshot,
    step: StepId,
) -> Result<ExecutionDto, ApiError> {
    let record = snapshot
        .step(step)
        .filter(|record| record.step.kind == StepKind::Execution)
        .ok_or(ApiError::NotFound)?;
    let row = snapshot
        .execution_of(step)
        .map(|stored| &stored.value)
        .ok_or(ApiError::NotFound)?;
    let risk = risk_of(state, row.host, row.contract.skill_name().as_ref());
    Ok(ExecutionDto::of(snapshot, record, row, risk))
}

/// Operations, newest first.
#[utoipa::path(
    get, path = "/api/v1/operations", operation_id = "list_operations", tag = "operations",
    params(OperationQuery),
    responses(
        (status = 200, description = "One page", body = OperationPage),
        (status = 400, description = "Unknown status or kind, or an incident id that does not parse", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<OperationQuery>,
) -> Result<Json<OperationPage>, ApiError> {
    let as_of_seq = snapshot_seq(&state).await?;
    if !state.services().operations.enabled() {
        return Ok(Json(OperationPage {
            items: Vec::new(),
            next_cursor: None,
            as_of_seq,
        }));
    }
    if let Some(status) = &query.status
        && !OperationStatus::NAMES.contains(&status.as_str())
    {
        return Err(ApiError::BadRequest(format!("unknown status `{status}`")));
    }
    let kind = match &query.kind {
        Some(kind) if kind == OperationKind::Investigate.as_str() => {
            Some(OperationKind::Investigate)
        }
        Some(kind) => return Err(ApiError::BadRequest(format!("unknown kind `{kind}`"))),
        None => None,
    };
    let incident = match &query.incident {
        Some(id) => Some(
            id.parse()
                .map_err(|_| ApiError::BadRequest(format!("`{id}` is not an incident id")))?,
        ),
        None => None,
    };
    let host = match &query.host {
        Some(name) => Some(host_named(&state, name).await?.id),
        None => None,
    };
    let filter = OperationFilter {
        status: query.status.clone(),
        incident,
        host,
        kind,
    };
    let got = state
        .deps
        .operation_repo
        .list(&filter, &page(query.cursor, query.limit))
        .await?;
    let mut items = Vec::with_capacity(got.items.len());
    for operation in &got.items {
        let loaded = state.deps.operation_repo.load(operation.id).await?;
        items.push(OperationSummaryDto::of(operation, loaded.as_ref()));
    }
    super::experience::decorate(&state, &mut items).await;
    Ok(Json(OperationPage {
        items,
        next_cursor: got.next_cursor,
        as_of_seq,
    }))
}

/// One operation with its trace and executions.
#[utoipa::path(
    get, path = "/api/v1/operations/{id}", operation_id = "get_operation", tag = "operations",
    params(("id" = String, Path, description = "Operation id")),
    responses(
        (status = 200, description = "The operation", body = OperationDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled", body = ErrorBody),
    )
)]
pub(super) async fn detail(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<OperationDetailDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let id: OperationId = parse(&id)?;
    let as_of_seq = snapshot_seq(&state).await?;
    let snapshot = snapshot(&state, id).await?;
    let mut executions = Vec::new();
    for record in &snapshot.steps {
        if record.step.kind == StepKind::Execution {
            executions.push(execution(&state, &snapshot, record.step.id)?);
        }
    }
    let mut row = [OperationSummaryDto::of(
        &snapshot.operation,
        Some(&snapshot),
    )];
    super::experience::decorate(&state, &mut row).await;
    let [operation] = row;
    Ok(Json(OperationDetailDto {
        as_of_seq,
        operation,
        trace: OperationTraceDto::from(&build_trace(&snapshot, as_of_seq)),
        executions,
    }))
}

/// The timeline and graph of one operation.
#[utoipa::path(
    get, path = "/api/v1/operations/{id}/trace", operation_id = "get_operation_trace", tag = "operations",
    params(("id" = String, Path, description = "Operation id")),
    responses(
        (status = 200, description = "The trace", body = OperationTraceDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled", body = ErrorBody),
    )
)]
pub(super) async fn trace(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<OperationTraceDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let id: OperationId = parse(&id)?;
    let as_of_seq = snapshot_seq(&state).await?;
    let snapshot = snapshot(&state, id).await?;
    Ok(Json(OperationTraceDto::from(&build_trace(
        &snapshot, as_of_seq,
    ))))
}

/// One step with its stored intent and output.
#[utoipa::path(
    get, path = "/api/v1/operations/{id}/steps/{step}", operation_id = "get_operation_step", tag = "operations",
    params(
        ("id" = String, Path, description = "Operation id"),
        ("step" = String, Path, description = "Step id"),
    ),
    responses(
        (status = 200, description = "The step", body = StepDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation, or no such step in it", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled", body = ErrorBody),
    )
)]
pub(super) async fn step(
    State(state): State<ApiState>,
    _auth: Authed,
    Path((id, step)): Path<(String, String)>,
) -> Result<Json<StepDetailDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let (id, step): (OperationId, StepId) = (parse(&id)?, parse(&step)?);
    let snapshot = snapshot(&state, id).await?;
    let record = snapshot.step(step).ok_or(ApiError::NotFound)?;
    let mut dto = StepDetailDto::from(record);
    let is_block = snapshot
        .execution_of(step)
        .is_some_and(|stored| stored.value.contract.as_str() == "firewall.block_source");
    // A turn an older build stored may still call a security event expected.
    if let Some(subject) = snapshot.operation.pinned.first() {
        let title = snapshot.operation.goal.trim_start_matches("Investigate: ");
        dto.show_security(&subject.check, title);
    }
    if is_block {
        let view = state
            .services()
            .containment
            .step_view(id, step)
            .await
            .map_err(super::containment::containment_error)?;
        dto.containment = match (&view.block, &view.preview) {
            (Some(block), _) => Some(super::dto_containment::ContainmentStepDto::of_block(block)),
            (None, Some(preview)) => Some(super::dto_containment::ContainmentStepDto::of_preview(
                preview,
            )),
            (None, None) => None,
        };
    }
    Ok(Json(dto))
}

/// Approves an Execution step. The body names the v2 plan hash shown on the execution; a
/// destructive skill, or one whose risk is not known, needs `confirm_text` equal to its name.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/steps/{step}/approve", operation_id = "approve_operation_step", tag = "operations",
    params(
        ("id" = String, Path, description = "Operation id"),
        ("step" = String, Path, description = "Execution step id"),
    ),
    request_body = ApproveStepRequest,
    responses(
        (status = 200, description = "Approved; it runs next", body = ExecutionDto),
        (status = 400, description = "No valid plan hash", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Needs the typed confirmation", body = ErrorBody),
        (status = 404, description = "No such operation or execution", body = ErrorBody),
        (status = 409, description = "Expired, already decided, or the plan hash does not match", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled, or the service is restarting", body = ErrorBody),
    )
)]
pub(super) async fn approve_step(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path((id, step)): Path<(String, String)>,
    Json(req): Json<ApproveStepRequest>,
) -> Result<Json<ExecutionDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let (id, step): (OperationId, StepId) = (parse(&id)?, parse(&step)?);
    let loaded = snapshot(&state, id).await?;
    execution(&state, &loaded, step)?;
    let plan_hash = req
        .plan_hash
        .as_deref()
        .and_then(|text| text.parse::<PlanHash>().ok())
        .ok_or_else(|| {
            ApiError::BadRequest(
                "plan_hash is required: 64 hex characters, as shown on the execution".to_owned(),
            )
        })?;
    let is_block = loaded
        .execution_of(step)
        .is_some_and(|stored| stored.value.contract.as_str() == "firewall.block_source");
    if is_block {
        // A block is approved with the password and the source typed out, on the web only.
        let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
        state
            .services()
            .containment
            .approve(
                &auth.caller(),
                id,
                step,
                ContainmentApproveInput {
                    plan_hash,
                    typed_source: req.confirm_text.as_deref(),
                    acknowledged: req.unverified_acknowledged.unwrap_or(false),
                },
                StepUp {
                    ip,
                    password: req.password.as_deref(),
                },
            )
            .await
            .map_err(super::containment::containment_error)?;
        let refreshed = snapshot(&state, id).await?;
        return Ok(Json(execution(&state, &refreshed, step)?));
    }
    state
        .services()
        .operations
        .approve_step(
            &auth.caller(),
            id,
            step,
            PlanBinding::Full(plan_hash),
            req.confirm_text.as_deref(),
            None,
        )
        .await
        .map_err(command_error)?;
    let refreshed = snapshot(&state, id).await?;
    Ok(Json(execution(&state, &refreshed, step)?))
}

/// Denies an Execution step.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/steps/{step}/deny", operation_id = "deny_operation_step", tag = "operations",
    params(
        ("id" = String, Path, description = "Operation id"),
        ("step" = String, Path, description = "Execution step id"),
    ),
    responses(
        (status = 200, description = "Denied", body = ExecutionDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation or execution", body = ErrorBody),
        (status = 409, description = "Already decided", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled, or the service is restarting", body = ErrorBody),
    )
)]
pub(super) async fn deny_step(
    State(state): State<ApiState>,
    auth: Authed,
    Path((id, step)): Path<(String, String)>,
) -> Result<Json<ExecutionDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let (id, step): (OperationId, StepId) = (parse(&id)?, parse(&step)?);
    let loaded = snapshot(&state, id).await?;
    execution(&state, &loaded, step)?;
    state
        .services()
        .operations
        .deny_step(&auth.caller(), id, step, None)
        .await
        .map_err(command_error)?;
    let refreshed = snapshot(&state, id).await?;
    Ok(Json(execution(&state, &refreshed, step)?))
}

/// The operations about `incident`, newest first.
pub(super) async fn for_incident(
    state: &ApiState,
    incident: domovoi_core::IncidentId,
) -> Result<Vec<OperationSummaryDto>, ApiError> {
    let mut found = state.deps.operation_repo.for_incident(incident).await?;
    found.sort_by_key(|operation| std::cmp::Reverse(operation.created_at));
    let mut rows = Vec::with_capacity(found.len());
    for operation in &found {
        let loaded = state.deps.operation_repo.load(operation.id).await?;
        rows.push(OperationSummaryDto::of(operation, loaded.as_ref()));
    }
    super::experience::decorate(state, &mut rows).await;
    Ok(rows)
}
