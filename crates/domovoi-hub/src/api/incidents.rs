//! Incidents: list, detail, ack, mute, unmute, dismiss and follow-up questions.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domovoi_core::ports::{ActionFilter, IncidentFilter, Page};
use domovoi_core::{
    AckRule, AckRuleId, Incident, IncidentId, IncidentStatus, Severity, TargetPattern,
    TriageOutcome,
};
use domovoi_engine::CommandError;
use domovoi_engine::cards::{CardLinks, build_card};
use domovoi_engine::triage::TriageError;
use jiff::SignedDuration;

use super::dto::{
    ActionDto, AskRequest, AskResponse, CardDto, DismissRequest, IncidentDetail, IncidentPage,
    IncidentQuery, IncidentSummary, MuteRequest, ReportDto, RetryQuery, TimelineEntryDto,
    TriageStateDto,
};
use super::error::{ApiError, ErrorBody};
use super::rules;
use super::tool_usage::tool_usage;
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
        .filter(|e| {
            e.kind.starts_with("triage.") && e.kind != "triage.followup" && e.kind != "triage.tools"
        })
        .find_map(|e| serde_json::from_value(e.detail.clone()).ok())
}

/// How a triage run went, as the API shows it.
fn triage_state(outcome: &TriageOutcome) -> TriageStateDto {
    match outcome {
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
    }
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
    let triage = outcome.as_ref().map(triage_state);
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
        tools_used: tool_usage(&timeline),
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

/// Dismisses an incident as a known condition: closes it now with the verdict `EXPECTED`,
/// and when `ignore_for_minutes` is given, creates the rule that keeps the check quiet
/// (0 means always). The rule is stored before the incident closes, so a check firing in
/// the moment between the two is already silenced, and a dismiss that fails afterwards
/// leaves a rule that a human can see and delete on the Known conditions page. The rule
/// matches the incident's host, check and target; `note` is the reason, so a human can
/// read later what it covers. A target with `*` in it cannot be matched exactly by a rule
/// pattern, so dismissing such an incident with a period is refused.
#[utoipa::path(
    post, path = "/api/v1/incidents/{id}/dismiss", operation_id = "dismiss_incident", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    request_body = DismissRequest,
    responses(
        (status = 204, description = "Dismissed"),
        (status = 400, description = "Empty reason, too long, too many minutes, or a target a rule cannot match", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No active incident", body = ErrorBody),
        (status = 409, description = "Not possible now", body = ErrorBody),
    )
)]
pub(super) async fn dismiss(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
    Json(req): Json<DismissRequest>,
) -> Result<StatusCode, ApiError> {
    let reason = req.reason.trim();
    if reason.is_empty() || reason.chars().count() > rules::MAX_NOTE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "the reason must have 1 to {} characters",
            rules::MAX_NOTE_CHARS
        )));
    }
    if req
        .ignore_for_minutes
        .is_some_and(|mins| mins > rules::MAX_EXPIRY_MINUTES)
    {
        return Err(ApiError::BadRequest(format!(
            "minutes must be between 0 and {}",
            rules::MAX_EXPIRY_MINUTES
        )));
    }
    // The incident is loaded first: the rule copies its host, check and target. A resolved
    // incident is not active, so there is nothing to dismiss; checking that before the rule
    // is stored means a 404 stores nothing.
    let incident = load(&state, &id).await?;
    if incident.status() == IncidentStatus::Resolved {
        return Err(ApiError::NotFound);
    }
    // The rule is stored before the incident closes, so a check firing in the moment
    // between the two is already silenced, which is what "keep this quiet" asks for. A
    // dismiss that fails after this leaves the rule in place: it shows on the Known
    // conditions page with its note, where a human can read it and delete it.
    let now = state.deps.clock.now();
    let rule = match req.ignore_for_minutes {
        None => None,
        Some(mins) => {
            // A target key allows `*` anywhere, a rule pattern only at the ends, so the
            // target of this incident may not be expressible. Silencing more than this one
            // condition is not what the dialog promises, so the dismiss is refused.
            if incident
                .target()
                .is_some_and(|target| target.as_str().contains('*'))
            {
                return Err(ApiError::BadRequest(
                    "the target contains `*`, which a rule cannot match exactly; dismiss with no period, \
                     or write the rule yourself on the Known conditions page"
                        .to_owned(),
                ));
            }
            let expires_at = match mins {
                0 => None,
                mins => Some(
                    now.checked_add(SignedDuration::from_mins(i64::from(mins)))
                        .map_err(|e| ApiError::BadRequest(e.to_string()))?,
                ),
            };
            let rule = AckRule {
                id: AckRuleId::new(),
                host: Some(incident.host()),
                check: incident.check().clone(),
                target_pattern: incident
                    .target()
                    .map(|target| TargetPattern::parse(target.as_str()))
                    .transpose()
                    .map_err(|e| ApiError::internal("target pattern", e))?,
                predicates: Vec::new(),
                note: reason.to_owned(),
                expires_at,
            };
            state.deps.acks.put_rule(&rule).await?;
            Some(rule)
        }
    };
    state
        .deps
        .manager
        .dismiss(incident.id(), current.user.id, reason.to_owned())
        .await
        .map_err(command)?;
    if let Some(rule) = &rule {
        rules::audit_rule_created(&state, &current, rule, now).await?;
    }
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
        .clone()
        .ok_or_else(|| ApiError::Unavailable("triage is not enabled".to_owned()))?;
    let incident = parse_id(&id)?;
    let question = question.to_owned();
    let user = current.user.id;
    // A task of its own, so that a closed tab or a navigation does not cancel the model
    // call halfway through the conversation.
    let answer = tokio::spawn(async move { triage.follow_up(incident, &question, user).await })
        .await
        .map_err(|e| ApiError::internal("follow-up failed", e))?
        .map_err(|e| match e {
            TriageError::UnknownIncident(_) => ApiError::NotFound,
            TriageError::Provider(err) => ApiError::Unavailable(err.to_string()),
            TriageError::Store(err) => ApiError::internal("follow-up failed", err),
        })?;
    Ok(Json(AskResponse { answer }))
}

/// Runs triage again on an incident, outside the queue but with the same caps, and reports
/// how it went. A report already cached for unchanged evidence is reused unless `fresh` is
/// true.
#[utoipa::path(
    post, path = "/api/v1/incidents/{id}/triage", operation_id = "retry_triage", tag = "incidents",
    params(("id" = String, Path, description = "Incident id"), RetryQuery),
    responses(
        (status = 200, description = "Triaged again; how it went", body = TriageStateDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such incident", body = ErrorBody),
        (status = 503, description = "Triage is not enabled", body = ErrorBody),
    )
)]
pub(super) async fn retry_triage(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
    Query(query): Query<RetryQuery>,
) -> Result<Json<TriageStateDto>, ApiError> {
    let triage = state
        .deps
        .triage
        .clone()
        .ok_or_else(|| ApiError::Unavailable("triage is not enabled".to_owned()))?;
    let incident = load(&state, &id).await?;
    let fresh = query.fresh == Some(true);
    // A task of its own, so that a closed tab or a navigation does not cancel the run; the
    // outcome is attached to the incident either way.
    let outcome = tokio::spawn(async move {
        if fresh {
            triage.retriage(&incident).await
        } else {
            triage.triage(&incident).await
        }
    })
    .await
    .map_err(|e| ApiError::internal("triage failed", e))?;
    Ok(Json(triage_state(&outcome)))
}
