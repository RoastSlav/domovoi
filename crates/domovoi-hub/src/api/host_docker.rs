//! A host's container runtime: what its agent found, and the hub's choice to switch it on
//! or off from the UI (ADR 0018).

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::settings_write::api_error;
use super::{ApiState, Authed, host_named};
use crate::settings::{HostDocker, SettingsService};

/// A host's container runtime.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HostDockerDto {
    /// Whether the agent is connected. Nothing below is known while it is not.
    pub connected: bool,
    /// Whether the agent reports its container runtime. False for an agent too old to.
    pub reported: bool,
    /// The engine found, such as `Docker` or `Podman`; empty when none.
    pub engine: String,
    /// The endpoint in use, or the one that failed; empty when there was none.
    pub endpoint: String,
    /// Whether an engine answered on the host, so it can be switched on.
    pub available: bool,
    /// Whether the agent's own config fixes the setting, so the hub cannot change it.
    pub locked: bool,
    /// Whether the container checks and tools are in use on the host now.
    pub enabled: bool,
    /// The hub's saved choice; absent when the agent decides.
    pub choice: Option<bool>,
    /// Why no engine can be used, in a sentence.
    pub problem: Option<String>,
}

/// The hub's choice for a host's container runtime.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct HostDockerUpdate {
    /// `true` or `false` switches the runtime on or off; `null` leaves it to the agent.
    pub enabled: Option<bool>,
}

/// Whether the agent got the change now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HostDockerSaved {
    /// True when sent to the connected agent; false when it gets it on reconnect.
    pub delivered: bool,
}

fn service(state: &ApiState) -> Result<&SettingsService, ApiError> {
    state
        .deps
        .settings
        .as_ref()
        .ok_or_else(|| ApiError::Unavailable("settings are read-only in this mode".to_owned()))
}

fn dto(found: &HostDocker) -> HostDockerDto {
    let report = found.report.as_ref();
    HostDockerDto {
        connected: found.connected,
        reported: report.is_some(),
        engine: report.map(|r| r.engine.clone()).unwrap_or_default(),
        endpoint: report.map(|r| r.endpoint.clone()).unwrap_or_default(),
        available: report.is_some_and(|r| r.available),
        locked: report.is_some_and(|r| r.locked),
        enabled: found.enabled(),
        choice: found.choice,
        problem: report.and_then(|r| r.problem.clone()),
    }
}

/// A host's container runtime: what its agent found and the hub's choice.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/docker", operation_id = "host_docker", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The container runtime", body = HostDockerDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn host_docker(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<HostDockerDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let found = service(&state)?.host_docker(&host).await;
    Ok(Json(dto(&found)))
}

/// Switches a host's container runtime on or off from the hub, or leaves it to the agent;
/// no restart. Refused when the agent's config fixes the setting or no engine was found.
#[utoipa::path(
    put, path = "/api/v1/hosts/{name}/docker", operation_id = "set_host_docker", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    request_body = HostDockerUpdate,
    responses(
        (status = 200, description = "Saved", body = HostDockerSaved),
        (status = 400, description = "Refused; `issues` says why", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn set_host_docker(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(name): Path<String>,
    Json(req): Json<HostDockerUpdate>,
) -> Result<Json<HostDockerSaved>, ApiError> {
    let host = host_named(&state, &name).await?;
    let delivered = service(&state)?
        .set_host_docker(&current.user, &host, req.enabled)
        .await
        .map_err(api_error)?;
    Ok(Json(HostDockerSaved { delivered }))
}
