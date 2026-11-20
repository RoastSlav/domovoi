//! Past operations: the summary of one operation with its provenance, the owner's edit of a
//! narrative field, similar past operations, and the owner's search. Everything here is
//! historical evidence for a person; none of it authorizes anything.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use domovoi_core::experience::{
    ExperienceQuery, ExperienceScope, SimilarQuery, SummaryField, SummaryRevision,
};
use domovoi_core::{HostId, OperationId, OperationSnapshot, StepKind};

use super::dto_experience::{
    EditSummaryFieldRequest, EditSummaryResponse, ExperienceItemDto, ExperienceSearchDto,
    HISTORY_LIMIT, HostRefDto, ResolutionBriefDto, ResolutionSummaryDto, SearchQueryParams,
    SimilarOperationDto, SimilarOperationsDto, SimilarQueryParams, SummaryFieldDto,
    SummaryRevisionDto, label,
};
use super::dto_operations::OperationSummaryDto;
use super::error::{ApiError, ErrorBody};
use super::operation_errors::operation_error;
use super::snapshot::snapshot_seq;
use super::{ApiState, Authed, host_named};
use crate::services::SummaryCommandError;
use domovoi_engine::operations::{EditSummaryError, SummaryError};

/// The longest search text, in characters.
const MAX_QUERY_CHARS: usize = 200;

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, ApiError> {
    text.parse().map_err(|_| ApiError::NotFound)
}

fn store(context: &str) -> impl FnOnce(domovoi_core::ports::StoreError) -> ApiError + '_ {
    move |err| ApiError::internal(context, err)
}

/// Host names by id, for the lists that show them.
async fn host_names(state: &ApiState) -> Result<BTreeMap<HostId, String>, ApiError> {
    Ok(state
        .deps
        .hosts
        .list_hosts()
        .await?
        .into_iter()
        .map(|host| (host.id, host.name.to_string()))
        .collect())
}

/// Adds the outcome of each finished operation to its row. One read per response; when it
/// fails the rows keep `null` and the page is still right.
pub(super) async fn decorate(state: &ApiState, rows: &mut [OperationSummaryDto]) {
    let ids: Vec<OperationId> = rows.iter().filter_map(|row| row.id.parse().ok()).collect();
    if ids.is_empty() {
        return;
    }
    match state.deps.experience.briefs(&ids).await {
        Ok(briefs) => {
            for row in rows {
                if let Some(brief) = row
                    .id
                    .parse::<OperationId>()
                    .ok()
                    .and_then(|id| briefs.get(&id))
                {
                    row.resolution = Some(ResolutionBriefDto::from(brief));
                }
            }
        }
        Err(err) => tracing::warn!(%err, "could not read the resolution briefs"),
    }
}

fn history_of(
    mut revisions: Vec<SummaryRevision>,
    state: &ApiState,
) -> (Vec<SummaryRevisionDto>, bool) {
    revisions.sort_by_key(|r| (r.created_at, r.field, r.revision));
    let truncated = revisions.len() > HISTORY_LIMIT;
    let skip = revisions.len().saturating_sub(HISTORY_LIMIT);
    let history = revisions
        .iter()
        .skip(skip)
        .map(|r| SummaryRevisionDto::of(r, state.deps.redactor.as_ref()))
        .collect();
    (history, truncated)
}

/// The summary of one operation: each field's current revision with its source, and the
/// whole history.
#[utoipa::path(
    get, path = "/api/v1/operations/{id}/summary", operation_id = "get_operation_summary", tag = "experience",
    params(("id" = String, Path, description = "Operation id")),
    responses(
        (status = 200, description = "The summary", body = ResolutionSummaryDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
    )
)]
pub(super) async fn get_operation_summary(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<ResolutionSummaryDto>, ApiError> {
    let id: OperationId = parse(&id)?;
    let as_of_seq = snapshot_seq(&state).await?;
    let operation = state
        .deps
        .operation_repo
        .get(id)
        .await?
        .ok_or(ApiError::NotFound)?
        .value;
    let terminal = operation.status.is_terminal();
    let current = state
        .deps
        .experience
        .current(id)
        .await
        .map_err(store("reading the summary"))?;
    let revisions = state
        .deps
        .experience
        .revisions(id)
        .await
        .map_err(store("reading the summary history"))?;
    let redactor = state.deps.redactor.as_ref();
    let fixed = if !terminal {
        "not_ready"
    } else if current.is_empty() {
        "drafting"
    } else {
        "ready"
    };
    let (history, history_truncated) = history_of(revisions, &state);
    Ok(Json(ResolutionSummaryDto {
        operation_id: id.to_string(),
        as_of_seq,
        state: fixed.to_owned(),
        ended_at: operation.ended_at.map(|t| t.to_string()),
        fields: current
            .iter()
            .map(|r| SummaryFieldDto::of(r, terminal, redactor))
            .collect(),
        history,
        history_truncated,
        label: label(),
    }))
}

/// What a refused summary edit means for the caller.
fn summary_error(err: SummaryCommandError) -> ApiError {
    match err {
        SummaryCommandError::NotEnabled => {
            ApiError::Unavailable("orchestration is not enabled".to_owned())
        }
        SummaryCommandError::Edit(EditSummaryError::Operation(err)) => operation_error(err),
        SummaryCommandError::Edit(EditSummaryError::Summary(refused)) => match refused {
            SummaryError::CodeOwned(_) => ApiError::Forbidden(refused.to_string()),
            SummaryError::UnknownField
            | SummaryError::Empty
            | SummaryError::TooLong { .. }
            | SummaryError::BadText => ApiError::BadRequest(refused.to_string()),
            SummaryError::NotTerminal => ApiError::Conflict(refused.to_string()),
            SummaryError::Stale { .. } => {
                ApiError::Conflict(format!("{}; reload and try again", refused))
            }
        },
    }
}

/// Writes or confirms the problem, cause or fix of a finished operation's summary.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/summary/{field}", operation_id = "edit_operation_summary_field", tag = "experience",
    params(
        ("id" = String, Path, description = "Operation id"),
        ("field" = String, Path, description = "`problem`, `cause` or `fix`"),
    ),
    request_body = EditSummaryFieldRequest,
    responses(
        (status = 200, description = "The revision that now holds the text", body = EditSummaryResponse),
        (status = 400, description = "Not a summary field, or the text is empty, too long or has control characters", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "A field Domovoi fills itself", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
        (status = 409, description = "The operation has not finished, or the summary changed since it was opened", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled, or the service is restarting", body = ErrorBody),
    )
)]
pub(super) async fn edit_operation_summary_field(
    State(state): State<ApiState>,
    auth: Authed,
    Path((id, field)): Path<(String, String)>,
    Json(req): Json<EditSummaryFieldRequest>,
) -> Result<Json<EditSummaryResponse>, ApiError> {
    let id: OperationId = parse(&id)?;
    let field = SummaryField::parse(&field)
        .ok_or_else(|| ApiError::BadRequest(SummaryError::UnknownField.to_string()))?;
    let edited = state
        .services()
        .operations
        .edit_summary_field(&auth.caller(), id, field, &req.value, req.expected_revision)
        .await
        .map_err(summary_error)?;
    let terminal = true;
    Ok(Json(EditSummaryResponse {
        field: SummaryFieldDto::of(&edited.revision, terminal, state.deps.redactor.as_ref()),
        unchanged: edited.unchanged,
    }))
}

fn limit_of(limit: Option<u16>, default: u16, max: u16) -> u16 {
    limit.unwrap_or(default).clamp(1, max)
}

async fn similar_dto(
    state: &ApiState,
    as_of_seq: u64,
    query: SimilarQuery,
) -> Result<SimilarOperationsDto, ApiError> {
    let hits = state
        .deps
        .experience
        .similar(&query)
        .await
        .map_err(store("reading similar operations"))?;
    let names = host_names(state).await?;
    let redactor = state.deps.redactor.as_ref();
    Ok(SimilarOperationsDto {
        items: hits
            .iter()
            .map(|hit| SimilarOperationDto::of(hit, &names, redactor))
            .collect(),
        as_of_seq,
        label: label(),
    })
}

fn contracts_of(snapshot: &OperationSnapshot) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for record in &snapshot.steps {
        if record.step.kind != StepKind::Execution {
            continue;
        }
        if let Some(row) = snapshot.execution_of(record.step.id) {
            let id = row.value.contract.to_string();
            if !found.contains(&id) {
                found.push(id);
            }
        }
    }
    found
}

/// Past operations that resemble an operation, on its host, best first.
#[utoipa::path(
    get, path = "/api/v1/operations/{id}/similar", operation_id = "get_similar_operations", tag = "experience",
    params(("id" = String, Path, description = "Operation id"), SimilarQueryParams),
    responses(
        (status = 200, description = "Similar past operations", body = SimilarOperationsDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
    )
)]
pub(super) async fn get_similar_operations(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
    Query(params): Query<SimilarQueryParams>,
) -> Result<Json<SimilarOperationsDto>, ApiError> {
    let id: OperationId = parse(&id)?;
    let as_of_seq = snapshot_seq(&state).await?;
    let snapshot = state
        .deps
        .operation_repo
        .load(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let Some(first) = snapshot.operation.pinned.first() else {
        return Ok(Json(SimilarOperationsDto {
            items: Vec::new(),
            as_of_seq,
            label: label(),
        }));
    };
    let query = SimilarQuery {
        host: first.host,
        check: Some(first.check.clone()),
        target: first.target.clone(),
        text: snapshot.operation.goal.clone(),
        contracts: contracts_of(&snapshot),
        exclude: Some(id),
        limit: limit_of(params.limit, 5, 20),
    };
    Ok(Json(similar_dto(&state, as_of_seq, query).await?))
}

/// Past operations that fixed something like an incident, on its host, best first.
#[utoipa::path(
    get, path = "/api/v1/incidents/{id}/similar-operations", operation_id = "get_similar_operations_for_incident", tag = "experience",
    params(("id" = String, Path, description = "Incident id"), SimilarQueryParams),
    responses(
        (status = 200, description = "Similar past operations", body = SimilarOperationsDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such incident", body = ErrorBody),
    )
)]
pub(super) async fn get_similar_operations_for_incident(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
    Query(params): Query<SimilarQueryParams>,
) -> Result<Json<SimilarOperationsDto>, ApiError> {
    let id: domovoi_core::IncidentId = parse(&id)?;
    let as_of_seq = snapshot_seq(&state).await?;
    let incident = state
        .deps
        .incidents
        .get(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let query = SimilarQuery {
        host: incident.host(),
        check: Some(incident.check().clone()),
        target: incident.target().cloned(),
        text: incident.title().to_owned(),
        contracts: Vec::new(),
        exclude: None,
        limit: limit_of(params.limit, 5, 20),
    };
    Ok(Json(similar_dto(&state, as_of_seq, query).await?))
}

/// Searches finished operations. One host by default; every host only when asked.
#[utoipa::path(
    get, path = "/api/v1/experience/search", operation_id = "search_experience_owner", tag = "experience",
    params(SearchQueryParams),
    responses(
        (status = 200, description = "The matches", body = ExperienceSearchDto),
        (status = 400, description = "A bad scope, a missing host or a query that is too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn search_experience_owner(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(params): Query<SearchQueryParams>,
) -> Result<Json<ExperienceSearchDto>, ApiError> {
    let text = params.q.clone().unwrap_or_default();
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiError::BadRequest(
            "the search text has at most 200 characters".to_owned(),
        ));
    }
    let (scope, host) = match params.scope.as_deref().unwrap_or("host") {
        "host" => {
            let name = params.host.as_deref().ok_or_else(|| {
                ApiError::BadRequest("choose a host, or ask for a global search".to_owned())
            })?;
            let host = host_named(&state, name).await?;
            (ExperienceScope::Host(host.id), Some(host))
        }
        "global" => {
            if params.host.is_some() {
                return Err(ApiError::BadRequest(
                    "a global search does not take a host".to_owned(),
                ));
            }
            (ExperienceScope::Global, None)
        }
        other => {
            return Err(ApiError::BadRequest(format!(
                "`{other}` is not a scope: use host or global"
            )));
        }
    };
    let as_of_seq = snapshot_seq(&state).await?;
    let scope_name = if host.is_some() { "host" } else { "global" };
    let hits = state
        .deps
        .experience
        .search(&ExperienceQuery {
            scope,
            text: text.clone(),
            limit: limit_of(params.limit, 10, 50),
            exclude: None,
        })
        .await
        .map_err(store("searching past operations"))?;
    let names = host_names(&state).await?;
    let redactor = state.deps.redactor.as_ref();
    Ok(Json(ExperienceSearchDto {
        scope: scope_name.to_owned(),
        host: host.map(|h| HostRefDto {
            id: h.id.to_string(),
            name: Some(h.name.to_string()),
        }),
        query: redactor.redact(&text),
        items: hits
            .iter()
            .map(|hit| ExperienceItemDto::of(hit, &names, redactor))
            .collect(),
        as_of_seq,
        label: label(),
    }))
}
