//! Hosts: list with presence and open incidents, detail with digest facts, enrollment
//! invites, revocation and skill catalogues.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::ports::{IncidentFilter, Page, Presence};
use domovoi_core::{DigestFact, FactValue, HostKind, HostName};
use jiff::SignedDuration;

use super::dto_admin::{FactDto, HostDetailDto, HostDto, InviteDto, ParamDto, RevokeDto, SkillDto};
use super::error::{ApiError, ErrorBody};
use super::version::{HUB_VERSION, is_older};
use super::{ApiState, Authed, host_named};

/// How far back host detail looks for digest facts.
pub const FACT_WINDOW: SignedDuration = SignedDuration::from_hours(24);

/// The most unresolved incidents counted per host.
pub const OPEN_COUNT_CAP: u16 = 200;

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

/// The DTO of `host`, with its agent's presence and its count of unresolved incidents.
async fn host_dto(state: &ApiState, host: &domovoi_core::Host) -> Result<HostDto, ApiError> {
    let filter = IncidentFilter {
        host: Some(host.id),
        active: Some(true),
        ..IncidentFilter::default()
    };
    let page = Page {
        cursor: None,
        limit: OPEN_COUNT_CAP,
    };
    let open = state.deps.incidents.list(&filter, &page).await?.items.len();
    let mut dto = HostDto {
        id: host.id.to_string(),
        name: host.name.to_string(),
        kind: kind(host.kind).to_owned(),
        connected: false,
        version: None,
        connected_since: None,
        last_seen: None,
        outdated: false,
        open_incidents: u32::try_from(open).unwrap_or(u32::MAX),
    };
    match state.deps.agents.presence(host.id) {
        Presence::Connected { since, version } => {
            dto.connected = true;
            dto.outdated = is_older(&version, HUB_VERSION);
            dto.version = Some(version);
            dto.connected_since = Some(since.to_string());
        }
        Presence::Disconnected { last_seen } => {
            dto.last_seen = last_seen.map(|t| t.to_string());
        }
    }
    Ok(dto)
}

/// Every host with its agent's presence, sorted by name.
#[utoipa::path(
    get, path = "/api/v1/hosts", operation_id = "list_hosts", tag = "hosts",
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
    let mut dtos = Vec::with_capacity(hosts.len());
    for host in &hosts {
        dtos.push(host_dto(&state, host).await?);
    }
    Ok(Json(dtos))
}

fn fact_dto(fact: &DigestFact) -> FactDto {
    let value = match &fact.value {
        FactValue::Text(text) => text.clone(),
        FactValue::Number(n) => {
            let rounded = (n * 100.0).round() / 100.0;
            rounded.to_string()
        }
    };
    FactDto {
        key: fact.key.clone(),
        label: fact.label.clone(),
        value,
        unit: fact.unit.clone(),
        observed_at: fact.observed_at.to_string(),
    }
}

/// The latest fact per key from `facts` (oldest first), sorted by key.
fn latest_per_key(facts: &[DigestFact]) -> Vec<FactDto> {
    let mut latest: BTreeMap<&str, &DigestFact> = BTreeMap::new();
    for fact in facts {
        latest.insert(fact.key.as_str(), fact);
    }
    latest.values().map(|fact| fact_dto(fact)).collect()
}

/// Host `name` with its agent and the latest value of each digest fact from the last day.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}", operation_id = "get_host", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The host", body = HostDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn detail(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<HostDetailDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let now = state.deps.clock.now();
    let since = now.checked_sub(FACT_WINDOW).unwrap_or(now);
    // `until` is exclusive; include facts observed right now.
    let until = now.checked_add(SignedDuration::from_secs(1)).unwrap_or(now);
    let facts = state.deps.facts.list(host.id, since, until).await?;
    Ok(Json(HostDetailDto {
        host: host_dto(&state, &host).await?,
        facts: latest_per_key(&facts),
    }))
}

/// A one-time enrollment invite for host `name`, creating the host if it is new. The
/// token is shown only in this response.
#[utoipa::path(
    post, path = "/api/v1/hosts/{name}/enrollment", operation_id = "create_enrollment", tag = "hosts",
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
    post, path = "/api/v1/hosts/{name}/revoke", operation_id = "revoke_agent", tag = "hosts",
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
    get, path = "/api/v1/hosts/{name}/skills", operation_id = "list_skills", tag = "hosts",
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
