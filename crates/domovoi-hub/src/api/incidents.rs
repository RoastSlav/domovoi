//! Incidents: list, detail, ack, mute, unmute and follow-up questions.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domovoi_core::ports::{ActionFilter, IncidentFilter, Page};
use domovoi_core::{Incident, IncidentId, IncidentStatus, Severity, TriageOutcome};
use domovoi_engine::CommandError;
use domovoi_engine::cards::{CardLinks, build_card};
use domovoi_engine::triage::TriageError;
use jiff::SignedDuration;

use super::dto::{
    ActionDto, AskRequest, AskResponse, CardDto, IncidentDetail, IncidentPage, IncidentQuery,
    IncidentSummary, MuteRequest, ReportDto, TimelineEntryDto, TriageStateDto,
};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named, page};

/// The longest follow-up question, in characters.
pub const MAX_QUESTION_CHARS: usize = 1000;

/// The longest mute, in minutes (a week).
pub const MAX_MUTE_MINUTES: u32 = 7 * 24 * 60;

fn parse_id(id: &str) -> Result<IncidentId, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound)
}

fn severity(name: &str) -> Result<Severity, ApiError> {
    serde_json::from_value(serde_json::Value::String(name.to_owned()))
        .map_err(|_| ApiError::BadRequest(format!("unknown severity `{name}`")))
}

fn command(err: CommandError) -> ApiError {
    match err {
        CommandError::UnknownIncident(_) => ApiError::NotFound,
        CommandError::NotAllowed(why) => ApiError::Conflict(why),
        CommandError::Unavailable => {
            ApiError::Unavailable("the incident manager is not running".to_owned())
        }
        CommandError::Store(e) => ApiError::internal("incident command failed", e),
    }
}

/// Incidents, newest first, with optional filters.
#[utoipa::path(
    get, path = "/api/v1/incidents", operation_id = "list_incidents", tag = "incidents",
    params(IncidentQuery),
    responses(
        (status = 200, description = "One page", body = IncidentPage),
        (status = 400, description = "Unknown filter value", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<IncidentQuery>,
) -> Result<Json<IncidentPage>, ApiError> {
    if let Some(status) = &query.status
        && !IncidentStatus::NAMES.contains(&status.as_str())
    {
        return Err(ApiError::BadRequest(format!("unknown status `{status}`")));
    }
    let host = match &query.host {
        Some(name) => Some(host_named(&state, name).await?.id),
        None => None,
    };
    let filter = IncidentFilter {
        host,
        status: query.status.clone(),
        severity: query.severity.as_deref().map(severity).transpose()?,
        ..IncidentFilter::default()
    };
    let got = state
        .deps
        .incidents
        .list(&filter, &page(query.cursor, query.limit))
        .await?;
    Ok(Json(IncidentPage {
        items: got.items.iter().map(IncidentSummary::from).collect(),
        next_cursor: got.next_cursor,
    }))
}

async fn load(state: &ApiState, id: &str) -> Result<Incident, ApiError> {
    state
        .deps
        .incidents
        .get(parse_id(id)?)
        .await?
        .ok_or(ApiError::NotFound)
}

/// The latest triage outcome in the timeline.
pub(crate) fn latest_triage(
    timeline: &[domovoi_core::ports::IncidentTimelineEntry],
) -> Option<TriageOutcome> {
    timeline
        .iter()
        .rev()
        .filter(|e| e.kind.starts_with("triage.") && e.kind != "triage.followup")
        .find_map(|e| serde_json::from_value(e.detail.clone()).ok())
}

/// Everything about one incident: report, card, timeline and actions.
#[utoipa::path(
    get, path = "/api/v1/incidents/{id}", operation_id = "get_incident", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    responses(
        (status = 200, description = "The incident", body = IncidentDetail),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such incident", body = ErrorBody),
    )
)]
pub(super) async fn detail(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<IncidentDetail>, ApiError> {
    let incident = load(&state, &id).await?;
    let deps = &state.deps;
    let host = deps
        .hosts
        .get_host(incident.host())
        .await?
        .ok_or(ApiError::NotFound)?;
    let timeline = deps.incidents.timeline(incident.id()).await?;
    let outcome = latest_triage(&timeline);
    let report = match &outcome {
        Some(TriageOutcome::Report(r) | TriageOutcome::Cached { report: r, .. }) => Some(r.clone()),
        _ => None,
    };
    let triage = outcome.as_ref().map(|o| match o {
        TriageOutcome::Report(_) => TriageStateDto {
            kind: "report".to_owned(),
            reason: None,
        },
        TriageOutcome::Cached { .. } => TriageStateDto {
            kind: "cached".to_owned(),
            reason: None,
        },
        TriageOutcome::Skipped { reason } => TriageStateDto {
            kind: "skipped".to_owned(),
            reason: Some(reason.clone()),
        },
        TriageOutcome::Failed { reason } => TriageStateDto {
            kind: "failed".to_owned(),
            reason: Some(reason.clone()),
        },
    });
    let filter = ActionFilter {
        incident: Some(incident.id()),
        ..ActionFilter::default()
    };
    let actions = deps
        .actions
        .list(&filter, &Page::first(MAX_ACTIONS))
        .await?
        .items;
    let latest = deps
        .signals
        .list_for_incident(incident.id(), 1)
        .await?
        .into_iter()
        .next();
    let card = build_card(
        &incident,
        report.as_ref(),
        &actions,
        &host.name,
        &CardLinks::default(),
        latest.as_ref(),
    );
    Ok(Json(IncidentDetail {
        incident: IncidentSummary::from(&incident),
        host: host.name.to_string(),
        report: report.as_ref().map(ReportDto::from),
        triage,
        card: CardDto::from(&card),
        timeline: timeline.iter().map(TimelineEntryDto::from).collect(),
        actions: actions.iter().map(ActionDto::from).collect(),
    }))
}

/// The most actions shown on an incident.
const MAX_ACTIONS: u16 = 50;

/// Acknowledges an incident.
#[utoipa::path(
    post, path = "/api/v1/incidents/{id}/ack", operation_id = "ack_incident", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    responses(
        (status = 204, description = "Acknowledged"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No active incident", body = ErrorBody),
        (status = 409, description = "Not possible now", body = ErrorBody),
    )
)]
pub(super) async fn ack(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .deps
        .manager
        .ack(parse_id(&id)?, current.user.id)
        .await
        .map_err(command)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Mutes an incident for a while.
#[utoipa::path(
    post, path = "/api/v1/incidents/{id}/mute", operation_id = "mute_incident", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    request_body = MuteRequest,
    responses(
        (status = 204, description = "Muted"),
        (status = 400, description = "Bad duration", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No active incident", body = ErrorBody),
    )
)]
pub(super) async fn mute(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
    Json(req): Json<MuteRequest>,
) -> Result<StatusCode, ApiError> {
    if !(1..=MAX_MUTE_MINUTES).contains(&req.minutes) {
        return Err(ApiError::BadRequest(format!(
            "minutes must be between 1 and {MAX_MUTE_MINUTES}"
        )));
    }
    let now = state.deps.clock.now();
    let until = now
        .checked_add(SignedDuration::from_mins(i64::from(req.minutes)))
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    state
        .deps
        .manager
        .mute(parse_id(&id)?, until, current.user.id)
        .await
        .map_err(command)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lifts a mute.
#[utoipa::path(
    post, path = "/api/v1/incidents/{id}/unmute", operation_id = "unmute_incident", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    responses(
        (status = 204, description = "Unmuted"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No active incident", body = ErrorBody),
        (status = 409, description = "Not muted", body = ErrorBody),
    )
)]
pub(super) async fn unmute(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .deps
        .manager
        .unmute(parse_id(&id)?)
        .await
        .map_err(command)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Asks the triage model a follow-up question about an incident. It uses read-only tools
/// and never proposes actions.
#[utoipa::path(
    post, path = "/api/v1/incidents/{id}/ask", operation_id = "ask_incident", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    request_body = AskRequest,
    responses(
        (status = 200, description = "The answer", body = AskResponse),
        (status = 400, description = "Empty or too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such incident", body = ErrorBody),
        (status = 503, description = "Triage is off or the model is unavailable", body = ErrorBody),
    )
)]
pub(super) async fn ask(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
    Json(req): Json<AskRequest>,
) -> Result<Json<AskResponse>, ApiError> {
    let question = req.question.trim();
    if question.is_empty() || question.chars().count() > MAX_QUESTION_CHARS {
        return Err(ApiError::BadRequest(format!(
            "the question must have 1 to {MAX_QUESTION_CHARS} characters"
        )));
    }
    let triage = state
        .deps
        .triage
        .as_ref()
        .ok_or_else(|| ApiError::Unavailable("triage is not enabled".to_owned()))?;
    let answer = triage
        .follow_up(parse_id(&id)?, question, current.user.id)
        .await
        .map_err(|e| match e {
            TriageError::UnknownIncident(_) => ApiError::NotFound,
            TriageError::Provider(err) => ApiError::Unavailable(err.to_string()),
            TriageError::Store(err) => ApiError::internal("follow-up failed", err),
        })?;
    Ok(Json(AskResponse { answer }))
}
