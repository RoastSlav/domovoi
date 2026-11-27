//! Turning one host's skill on or off from the web UI.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
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
        (status = 409, description = "The host cannot run this skill (the message says why), so it cannot be enabled", body = ErrorBody),
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
    let Some(announced) = state
        .deps
        .agents
        .skills(host.id)
        .into_iter()
        .find(|s| s.name == skill)
    else {
        return Err(ApiError::NotFound);
    };
    if req.enabled
        && let Some(reason) = state
            .deps
            .capabilities
            .skill_cannot_run(host.id, &announced)
    {
        return Err(ApiError::Conflict(format!(
            "{skill_name} cannot be enabled: {}",
            reason.describe()
        )));
    }
    let kind = if req.enabled {
        "skill.enabled"
    } else {
        "skill.disabled"
    };
    let subject = format!("host:{host_name}/skill:{skill_name}");
    let detail = json!({ "enabled": req.enabled });
    let state_ref = &state;
    state
        .deps
        .capabilities
        .set_skill_enabled_with(&host, skill, req.enabled, |key, value| {
            super::audit_note::decide(
                state_ref,
                domovoi_core::ports::AdminWrite::Setting {
                    key,
                    value: Some(value),
                },
                current.user.id,
                kind,
                subject,
                detail,
            )
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
