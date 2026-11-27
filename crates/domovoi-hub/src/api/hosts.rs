//! Hosts: list with presence and open incidents, detail with digest facts, enrollment
//! invites, revocation and skill catalogues.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use domovoi_core::ports::{CapabilityRegistry, LinkError};
use domovoi_core::{DigestFact, FactValue, HostKind, HostName};
use jiff::SignedDuration;
use serde_json::json;

use super::audit_note::note;
use super::dto::{LogLineDto, LogsQuery};
use super::dto_admin::{
    FactDto, HostDetailDto, HostDto, InviteDto, ParamDto, RetiredFactDto, RevokeDto, RevokeRequest,
    SkillDto, VerifyDto,
};
use super::error::{ApiError, ErrorBody};
use super::logs::{DEFAULT_LEVEL, DEFAULT_LINES, MAX_LINES};
use super::{ApiState, Authed, host_named};

/// How far back host detail looks for digest facts.
pub const FACT_WINDOW: SignedDuration = SignedDuration::from_hours(24);

fn kind(kind: HostKind, in_container: bool) -> &'static str {
    match kind {
        HostKind::Local => "local",
        HostKind::Remote if in_container => "container_agent",
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
pub(crate) async fn host_dto(
    state: &ApiState,
    host: &domovoi_core::Host,
) -> Result<HostDto, ApiError> {
    let status = state.services().hosts.status(host).await?;
    Ok(HostDto {
        id: status.id.to_string(),
        name: status.name.to_string(),
        kind: kind(status.kind, status.in_container).to_owned(),
        connected: status.connected,
        agent_enrolled: status.agent_enrolled,
        version: status.version,
        connected_since: status.connected_since.map(|at| at.to_string()),
        last_seen: status.last_seen.map(|at| at.to_string()),
        outdated: status.outdated,
        protocol_state: status.protocol_state.to_owned(),
        reported_protocol: status.reported_protocol,
        hub_protocol: status.hub_protocol,
        reported_release: status.reported_release,
        incompatible_since: status.incompatible_since.map(|at| at.to_string()),
        open_incidents: status.open_incidents,
    })
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

/// The latest fact per key from `facts` (oldest first), sorted by key, split into the facts
/// that are current and those whose key `retired` says the agent stopped reporting after their
/// last sample.
fn latest_per_key(
    facts: &[DigestFact],
    retired: &[(String, jiff::Timestamp)],
) -> (Vec<FactDto>, Vec<RetiredFactDto>) {
    let mut latest: BTreeMap<&str, &DigestFact> = BTreeMap::new();
    for fact in facts {
        latest.insert(fact.key.as_str(), fact);
    }
    let mut current = Vec::new();
    let mut gone = Vec::new();
    for fact in latest.values() {
        let at = retired
            .iter()
            .find(|(key, at)| key == &fact.key && *at >= fact.observed_at)
            .map(|(_, at)| *at);
        match at {
            Some(at) => gone.push(RetiredFactDto {
                fact: fact_dto(fact),
                retired_at: at.to_string(),
            }),
            None => current.push(fact_dto(fact)),
        }
    }
    (current, gone)
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
    let retired = state.deps.facts.retired(host.id).await?;
    let (facts, retired_facts) = latest_per_key(&facts, &retired);
    Ok(Json(HostDetailDto {
        host: host_dto(&state, &host).await?,
        facts,
        retired_facts,
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
    Authed(current): Authed,
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
    note(
        &state,
        format!("user:{}", current.user.id),
        "host.invite_created",
        format!("host:{name}"),
        json!({ "expires_at": invite.expires_at.to_string() }),
    )
    .await;
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
    request_body = RevokeRequest,
    responses(
        (status = 200, description = "Whether an agent was revoked", body = RevokeDto),
        (status = 400, description = "The confirmation is not the host name", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn revoke(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(name): Path<String>,
    Json(body): Json<RevokeRequest>,
) -> Result<Json<RevokeDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    if body.confirm != host.name.as_str() {
        return Err(ApiError::BadRequest(format!(
            "type the host name ({}) in `confirm` to revoke its agent",
            host.name
        )));
    }
    let revoked = state
        .deps
        .agents
        .revoke(host.id)
        .await
        .map_err(agent_error)?;
    if revoked {
        tracing::info!(host = %host.name, "agent revoked");
        note(
            &state,
            format!("user:{}", current.user.id),
            "host.agent_revoked",
            format!("host:{}", host.name),
            json!({}),
        )
        .await;
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
            manifest_hash: s.manifest_hash.to_string(),
            verify: s.verify.map(|v| VerifyDto {
                recheck: v.recheck,
                delay_secs: v.delay.as_secs(),
            }),
            readme: s.readme,
            enabled: state
                .deps
                .capabilities
                .enabled(host.id, &domovoi_core::ContractId::skill(&s.name)),
        })
        .collect();
    Ok(Json(skills))
}

/// Why the browser cannot get an agent's log lines now: it is not connected, did not
/// answer, or runs a release too old to send them.
fn link_error(err: LinkError) -> ApiError {
    ApiError::Conflict(err.to_string())
}

/// The recent log lines host `name`'s agent keeps, scrubbed again here so the page is
/// safe even if the agent was not. An agent too old to send them is refused with a
/// message telling the person to update it.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/logs", operation_id = "list_host_logs", tag = "hosts",
    params(
        ("name" = String, Path, description = "Host name"),
        LogsQuery,
    ),
    responses(
        (status = 200, description = "The agent's recent log lines", body = Vec<LogLineDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 409, description = "The agent is not connected, did not answer, or is too old to send logs", body = ErrorBody),
    )
)]
pub(super) async fn logs(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
    query: Query<LogsQuery>,
) -> Result<Json<Vec<LogLineDto>>, ApiError> {
    let host = host_named(&state, &name).await?;
    let lines = query.lines.unwrap_or(DEFAULT_LINES).clamp(1, MAX_LINES);
    let min_level = query.level.as_deref().unwrap_or(DEFAULT_LEVEL).to_owned();
    let picked = state
        .deps
        .agents
        .logs(host.id, lines, min_level)
        .await
        .map_err(link_error)?;
    Ok(Json(
        picked
            .iter()
            .map(|line| LogLineDto {
                at: line.at.to_string(),
                level: line.level.clone(),
                target: line.target.clone(),
                message: state.deps.redactor.redact(&line.message),
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_kind_is_container_agent_for_a_remote_host_in_a_container() {
        assert_eq!(kind(HostKind::Remote, true), "container_agent");
        assert_eq!(kind(HostKind::Remote, false), "remote");
        assert_eq!(kind(HostKind::Local, true), "local");
    }

    use domovoi_core::{DigestFact, FactValue, HostId};
    use jiff::Timestamp;

    use super::latest_per_key;

    fn fact(host: HostId, key: &str, at: &str) -> DigestFact {
        DigestFact {
            host,
            key: key.to_owned(),
            label: key.to_owned(),
            value: FactValue::Number(1.0),
            unit: None,
            observed_at: at.parse().unwrap(),
        }
    }

    #[test]
    fn a_retired_key_leaves_the_current_facts_until_a_newer_sample_arrives() {
        let host = HostId::new();
        let samples = [
            fact(host, "disk./a", "2026-11-27T10:00:00Z"),
            fact(host, "disk./b", "2026-11-27T10:00:00Z"),
        ];
        let at: Timestamp = "2026-11-27T11:00:00Z".parse().unwrap();
        let retired = vec![("disk./b".to_owned(), at)];

        let (current, gone) = latest_per_key(&samples, &retired);
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].key, "disk./a");
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].fact.key, "disk./b");
        assert!(gone[0].retired_at.starts_with("2026-11-27T11:00:00"));

        // The key is reported again after it was retired: current again.
        let again = [
            samples[0].clone(),
            fact(host, "disk./b", "2026-11-27T12:00:00Z"),
        ];
        let (current, gone) = latest_per_key(&again, &retired);
        assert_eq!(current.len(), 2);
        assert!(gone.is_empty());
    }
}
