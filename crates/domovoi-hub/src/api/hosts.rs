//! Hosts: list with presence, enrollment invites, revocation and skill catalogues.

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::ports::Presence;
use domovoi_core::{HostKind, HostName};

use super::dto_admin::{HostDto, InviteDto, ParamDto, RevokeDto, SkillDto};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};

fn kind(kind: HostKind) -> &'static str {
    match kind {
        HostKind::Local => "local",
        HostKind::Remote => "remote",
        HostKind::External => "external",
    }
}

fn agent_error(err: crate::agent_server::AgentServerError) -> ApiError {
    match err {
        crate::agent_server::AgentServerError::Store(e) => ApiError::from(e),
        other => ApiError::internal("agent server failed", other),
    }
}

/// Every host with its agent's presence, sorted by name.
#[utoipa::path(
    get, path = "/api/v1/hosts", tag = "hosts",
    responses(
        (status = 200, description = "The hosts", body = Vec<HostDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<Vec<HostDto>>, ApiError> {
    let hosts = state.deps.hosts.list_hosts().await?;
    let dtos = hosts
        .iter()
        .map(|host| {
            let mut dto = HostDto {
                id: host.id.to_string(),
                name: host.name.to_string(),
                kind: kind(host.kind).to_owned(),
                connected: false,
                version: None,
                connected_since: None,
                last_seen: None,
            };
            match state.deps.agents.presence(host.id) {
                Presence::Connected { since, version } => {
                    dto.connected = true;
                    dto.version = Some(version);
                    dto.connected_since = Some(since.to_string());
                }
                Presence::Disconnected { last_seen } => {
                    dto.last_seen = last_seen.map(|t| t.to_string());
                }
            }
            dto
        })
        .collect();
    Ok(Json(dtos))
}

/// A one-time enrollment invite for host `name`, creating the host if it is new. The
/// token is shown only in this response.
#[utoipa::path(
    post, path = "/api/v1/hosts/{name}/enrollment", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The invite", body = InviteDto),
        (status = 400, description = "Invalid host name", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn enrollment(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<InviteDto>, ApiError> {
    let name = HostName::parse(&name).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let invite = state
        .deps
        .agents
        .create_invite(&name)
        .await
        .map_err(agent_error)?;
    tracing::info!(host = %name, "enrollment invite created");
    Ok(Json(InviteDto {
        host_id: invite.host.to_string(),
        token: invite.token.expose().to_owned(),
        expires_at: invite.expires_at.to_string(),
        fingerprint: invite.fingerprint.clone(),
        command: invite.command.clone(),
    }))
}

/// Revokes the agent of host `name` and disconnects it.
#[utoipa::path(
    post, path = "/api/v1/hosts/{name}/revoke", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "Whether an agent was revoked", body = RevokeDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn revoke(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<RevokeDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let revoked = state
        .deps
        .agents
        .revoke(host.id)
        .await
        .map_err(agent_error)?;
    if revoked {
        tracing::info!(host = %host.name, "agent revoked");
    }
    Ok(Json(RevokeDto { revoked }))
}

/// The skills host `name`'s agent reported.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/skills", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The catalogue", body = Vec<SkillDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn skills(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<Vec<SkillDto>>, ApiError> {
    let host = host_named(&state, &name).await?;
    let skills = state
        .deps
        .agents
        .skills(host.id)
        .into_iter()
        .map(|s| SkillDto {
            name: s.name.to_string(),
            description: s.description.clone(),
            risk: serde_json::to_value(s.risk)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            params: s
                .params
                .iter()
                .map(|p| ParamDto {
                    name: p.name.to_string(),
                    description: p.description.clone(),
                    required: p.required,
                    kind: serde_json::to_value(&p.kind).unwrap_or_default(),
                })
                .collect(),
            applies_to: s.applies_to.iter().map(ToString::to_string).collect(),
            timeout_secs: s.timeout.as_secs(),
        })
        .collect();
    Ok(Json(skills))
}
