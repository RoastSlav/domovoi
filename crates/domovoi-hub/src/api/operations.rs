//! Operations: the list, one operation with its trace and executions, one step, and the
//! approval and denial of an Execution step. Approving here is the only way a step becomes
//! authorized (INV-1): the plan hash must be the stored v2 hash, and a destructive skill (or
//! one whose risk is not known) needs its name typed exactly (INV-6).

use axum::Json;
use axum::extract::{Path, Query, State};
use domovoi_core::ports::OperationFilter;
use domovoi_core::{
    OperationId, OperationKind, OperationSnapshot, OperationStatus, PlanBinding, PlanHash,
    SkillName, StepId, StepKind, build_trace,
};
use domovoi_engine::operations::OperationError;

use super::dto::TriageStateDto;
use super::dto_operations::{
    ApproveStepRequest, ExecutionDto, OperationDetailDto, OperationPage, OperationQuery,
    OperationSummaryDto, OperationTraceDto, StepDetailDto,
};
use super::error::{ApiError, ErrorBody};
use super::snapshot::snapshot_seq;
use super::{ApiState, Authed, host_named, page};
use crate::auth::approval::{web_approval, web_authorization};

/// The 503 every operation endpoint but the list answers while orchestration is off.
fn off() -> ApiError {
    ApiError::Unavailable("orchestration is not enabled".to_owned())
}

/// What an operation command's refusal means for the caller.
pub(crate) fn operation_error(err: OperationError) -> ApiError {
    use domovoi_core::ExecutionError;
    match err {
        OperationError::NotFound(_) => ApiError::NotFound,
        OperationError::Execution(ExecutionError::PlanChanged) => ApiError::Conflict(
            "the plan changed since you opened it; reload the page and review it again".to_owned(),
        ),
        OperationError::Execution(ExecutionError::Expired) => {
            ApiError::Conflict("the approval window has closed".to_owned())
        }
        OperationError::Execution(ExecutionError::AlreadyAuthorized) => {
            ApiError::Conflict("this step was already approved".to_owned())
        }
        OperationError::Execution(ExecutionError::NotYetSupported) => ApiError::Conflict(
            "policy authorizations are not supported yet; a person must approve".to_owned(),
        ),
        OperationError::Decide(refused) => ApiError::Conflict(refused.to_string()),
        OperationError::Conflict | OperationError::StaleEffect => {
            ApiError::Conflict("another change landed first; reload and try again".to_owned())
        }
        OperationError::HostUnavailable(_) => {
            ApiError::Conflict("the host's agent is not connected".to_owned())
        }
        OperationError::Store(e) => ApiError::internal("operation change failed", e),
        OperationError::Unavailable => {
            ApiError::Unavailable("the operation service is restarting; try again".to_owned())
        }
    }
}

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, ApiError> {
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
    let skill = skill?;
    state
        .deps
        .agents
        .skills(host)
        .iter()
        .find(|summary| &summary.name == skill)
        .and_then(|summary| serde_json::to_value(summary.risk).ok())
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
    if state.deps.operations.is_none() {
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
    if state.deps.operations.is_none() {
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
    Ok(Json(OperationDetailDto {
        as_of_seq,
        operation: OperationSummaryDto::of(&snapshot.operation, Some(&snapshot)),
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
    if state.deps.operations.is_none() {
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
    if state.deps.operations.is_none() {
        return Err(off());
    }
    let (id, step): (OperationId, StepId) = (parse(&id)?, parse(&step)?);
    let snapshot = snapshot(&state, id).await?;
    let record = snapshot.step(step).ok_or(ApiError::NotFound)?;
    Ok(Json(StepDetailDto::from(record)))
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
    Authed(current): Authed,
    Path((id, step)): Path<(String, String)>,
    Json(req): Json<ApproveStepRequest>,
) -> Result<Json<ExecutionDto>, ApiError> {
    let operations = state.deps.operations.clone().ok_or_else(off)?;
    let (id, step): (OperationId, StepId) = (parse(&id)?, parse(&step)?);
    let loaded = snapshot(&state, id).await?;
    let shown = execution(&state, &loaded, step)?;
    let plan_hash = req
        .plan_hash
        .as_deref()
        .and_then(|text| text.parse::<PlanHash>().ok())
        .ok_or_else(|| {
            ApiError::BadRequest(
                "plan_hash is required: 64 hex characters, as shown on the execution".to_owned(),
            )
        })?;
    let skill = loaded
        .execution_of(step)
        .and_then(|row| row.value.contract.skill_name())
        .ok_or_else(|| ApiError::Conflict("only a skill can be approved from here".to_owned()))?;
    // Checked before anything is written: an unknown risk fails closed.
    if shown.confirm_required && req.confirm_text.as_deref() != Some(skill.as_str()) {
        return Err(ApiError::Forbidden(
            "type the skill's name to approve a destructive action".to_owned(),
        ));
    }
    let (authorization, provenance) = web_authorization(
        current.user.id,
        req.confirm_text.as_deref(),
        &skill,
        &current.session_hash,
        state.deps.clock.now(),
    );
    operations
        .authorize(
            id,
            step,
            authorization,
            PlanBinding::Full(plan_hash),
            provenance,
        )
        .await
        .map_err(operation_error)?;
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
    Authed(current): Authed,
    Path((id, step)): Path<(String, String)>,
) -> Result<Json<ExecutionDto>, ApiError> {
    let operations = state.deps.operations.clone().ok_or_else(off)?;
    let (id, step): (OperationId, StepId) = (parse(&id)?, parse(&step)?);
    let loaded = snapshot(&state, id).await?;
    execution(&state, &loaded, step)?;
    let skill = loaded
        .execution_of(step)
        .and_then(|row| row.value.contract.skill_name())
        .ok_or_else(|| ApiError::Conflict("only a skill can be denied from here".to_owned()))?;
    let approval = web_approval(current.user.id, None, &skill, state.deps.clock.now());
    operations
        .deny(id, step, approval)
        .await
        .map_err(operation_error)?;
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
    Ok(rows)
}

/// Investigates `incident` again as an operation and reports how the first outcome went.
/// A task of its own, so that a closed tab does not cancel the run.
pub(super) async fn retry(
    operations: domovoi_engine::operations::OperationHandle,
    incident: domovoi_core::Incident,
    fresh: bool,
) -> Result<Json<TriageStateDto>, ApiError> {
    use domovoi_engine::operations::InvestigateError;
    let result =
        tokio::spawn(async move { operations.investigate_and_wait(&incident, fresh).await })
            .await
            .map_err(|e| ApiError::internal("triage failed", e))?;
    match result {
        Ok(outcome) => Ok(Json(TriageStateDto::from(&outcome))),
        Err(InvestigateError::AlreadyRunning(_)) => Err(ApiError::Conflict(
            "an investigation is already running for this incident".to_owned(),
        )),
        Err(InvestigateError::TimedOut(_)) => Ok(Json(TriageStateDto {
            kind: "skipped".to_owned(),
            reason: Some(
                "the investigation is still running; its result appears on the incident".to_owned(),
            ),
        })),
        Err(InvestigateError::Operation(err)) => Err(operation_error(err)),
    }
}
