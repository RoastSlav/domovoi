//! The follow-up conversation of an incident: what the triage model still remembers,
//! and the command that forgets it.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use domovoi_core::{Incident, IncidentId};
use domovoi_engine::CommandError;
use domovoi_engine::triage::TriageError;

use super::dto::{FollowUpsDto, TurnDto};
use super::error::{ApiError, ErrorBody, busy};
use super::{ApiState, Authed};

fn parse_id(id: &str) -> Result<IncidentId, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound)
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

async fn load(state: &ApiState, id: &str) -> Result<Incident, ApiError> {
    state
        .deps
        .incidents
        .get(parse_id(id)?)
        .await?
        .ok_or(ApiError::NotFound)
}

/// The questions and answers the model still remembers for an incident, oldest first.
/// Answers recorded before the last clear are not in the list. An id that parses but is
/// not a stored incident gets 404, so a cleared conversation is never read as a
/// mistyped one.
#[utoipa::path(
    get, path = "/api/v1/incidents/{id}/followups", operation_id = "incident_followups", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    responses(
        (status = 200, description = "The remembered turns", body = FollowUpsDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Malformed or unknown incident id", body = ErrorBody),
        (status = 503, description = "Triage is off", body = ErrorBody),
    )
)]
pub(super) async fn history(
    State(state): State<ApiState>,
    Authed(_current): Authed,
    Path(id): Path<String>,
) -> Result<Json<FollowUpsDto>, ApiError> {
    let triage = state.deps.triage.clone().ok_or(ApiError::TriageOff)?;
    let turns = match triage.history(parse_id(&id)?).await {
        Ok(found) => found,
        Err(err) => {
            return Err(match err {
                TriageError::UnknownIncident(_) => ApiError::NotFound,
                TriageError::Provider(err) => ApiError::Unavailable(err),
                TriageError::BudgetSpent => ApiError::BudgetSpent,
                TriageError::Busy { retry_at } => busy(state.deps.clock.now(), retry_at),
                TriageError::Store(err) => ApiError::internal("follow-up history failed", err),
            });
        }
    };
    Ok(Json(FollowUpsDto {
        turns: turns.iter().map(TurnDto::from).collect(),
    }))
}

/// Forgets the conversation: the next question will not be shown the earlier ones. Works
/// whether triage is on or off, and on a resolved incident too; it writes one timeline
/// line and publishes nothing.
#[utoipa::path(
    post, path = "/api/v1/incidents/{id}/followups/clear", operation_id = "clear_incident_followups", tag = "incidents",
    params(("id" = String, Path, description = "Incident id")),
    responses(
        (status = 204, description = "The conversation was forgotten"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such incident", body = ErrorBody),
    )
)]
pub(super) async fn clear(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // The incident is loaded first so clearing an unknown id writes nothing; the store
    // does not check the incident when it appends a timeline entry.
    let incident = load(&state, &id).await?;
    state
        .deps
        .manager
        .clear_follow_ups(incident.id(), current.user.id)
        .await
        .map_err(command)?;
    Ok(StatusCode::NO_CONTENT)
}
