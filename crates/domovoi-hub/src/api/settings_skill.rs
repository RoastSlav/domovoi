//! Turning one host's skill on or off from the web UI.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use domovoi_core::ports::AuditEvent;
use domovoi_core::skill::SkillName;
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};

/// Turning one host skill on or off.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
pub struct SkillEnabledUpdate {
    /// Whether the hub may run and suggest the skill on this host.
    pub enabled: bool,
}

/// Turns one of a host's skills on or off. The switch is saved in the settings table and
/// takes effect immediately: a disabled skill stays on the host, but the hub never runs it
/// and never offers it to triage.
#[utoipa::path(
    put, path = "/api/v1/hosts/{name}/skills/{skill}", operation_id = "set_skill_enabled", tag = "hosts",
    params(
        ("name" = String, Path, description = "Host name"),
        ("skill" = String, Path, description = "Skill name"),
    ),
    request_body = SkillEnabledUpdate,
    responses(
        (status = 204, description = "Saved"),
        (status = 400, description = "Invalid skill name", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host, or the agent does not report that skill", body = ErrorBody),
    )
)]
pub(super) async fn set_skill_enabled(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path((host_name, skill_name)): Path<(String, String)>,
    Json(req): Json<SkillEnabledUpdate>,
) -> Result<StatusCode, ApiError> {
    let host = host_named(&state, &host_name).await?;
    let skill = SkillName::parse(&skill_name).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let announced = state
        .deps
        .agents
        .skills(host.id)
        .iter()
        .any(|s| s.name == skill);
    if !announced {
        return Err(ApiError::NotFound);
    }
    state
        .deps
        .capabilities
        .set_skill_enabled(&host, skill, req.enabled)
        .await?;
    let kind = if req.enabled {
        "skill.enabled".to_owned()
    } else {
        "skill.disabled".to_owned()
    };
    state
        .deps
        .audit
        .append(&AuditEvent {
            at: state.deps.clock.now(),
            actor: format!("user:{}", current.user.id),
            kind,
            subject: format!("host:{host_name}/skill:{skill_name}"),
            detail: json!({ "enabled": req.enabled }),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
