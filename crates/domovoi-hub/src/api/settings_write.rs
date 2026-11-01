//! Changing settings from the web UI (ADR 0017): save and apply, the schema for field
//! help, connection tests, and per-host check overrides. Every route needs an admin
//! session and, for writes, the CSRF token (INV-8). Secrets are write-only: requests
//! carry `{ keep }`, `{ set }` or `{ clear }`, and no response contains a secret value.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use domovoi_core::CheckKey;
use domovoi_proto::messages::CheckOverride;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody, IssueDto};
use super::{ApiState, Authed, host_named};
use crate::auth::proxy;
use crate::settings::{SettingsError, SettingsService, Target};

/// A change to the settings.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SettingsPatch {
    /// The revision the change was made on; a stale one answers 409.
    pub base_revision: u64,
    /// The changes, shaped like `hub.toml`. `null` removes a value so the lower layers
    /// apply. A secret takes `{ "keep": true }`, `{ "set": "…" }` or `{ "clear": true }`.
    #[schema(value_type = Object)]
    pub changes: Map<String, Value>,
    /// The admin's password, needed when the change touches chat approvers, trusted
    /// proxies, webhooks or any secret.
    #[serde(default)]
    pub password: Option<String>,
}

/// A saved change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SavedDto {
    /// The new revision.
    pub revision: u64,
    /// Whether the hub restarted with the change.
    pub applied: bool,
}

/// Candidate values for a connection test; nothing is saved.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TestRequest {
    /// The same shape as a save; a secret left as `{ "keep": true }` uses the stored one.
    #[schema(value_type = Object)]
    #[serde(default)]
    pub changes: Map<String, Value>,
}

/// The result of a connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TestResultDto {
    /// Whether it worked.
    pub ok: bool,
    /// What happened, in one line.
    pub message: String,
    /// How long it took, in milliseconds.
    pub latency_ms: u64,
}

/// One check's settings: on or off, interval, thresholds.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, ToSchema)]
pub struct CheckSettingsDto {
    /// Whether the check runs; `None` keeps the agent's value.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// How often, in seconds; `None` keeps the agent's value.
    #[serde(default)]
    pub interval_seconds: Option<u32>,
    /// Thresholds merged over the agent's.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub thresholds: BTreeMap<String, Value>,
}

/// One check on a host.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HostCheckDto {
    /// The check key.
    pub key: String,
    /// What the agent's own config sets, while it is connected.
    pub reported: Option<CheckSettingsDto>,
    /// What the hub overrides.
    pub overrides: Option<CheckSettingsDto>,
}

/// A host's checks.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HostChecksDto {
    /// Whether the agent is connected.
    pub connected: bool,
    /// Every check the agent reported or the hub overrides, by key.
    pub checks: Vec<HostCheckDto>,
}

/// New overrides for a host's checks, replacing the previous ones.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct HostChecksUpdate {
    /// Overrides by check key.
    pub overrides: BTreeMap<String, CheckSettingsDto>,
}

/// Whether the agent got the change now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HostChecksSaved {
    /// True when sent to the connected agent; false when it gets them on reconnect.
    pub delivered: bool,
}

fn service(state: &ApiState) -> Result<&SettingsService, ApiError> {
    state
        .deps
        .settings
        .as_ref()
        .ok_or_else(|| ApiError::Unavailable("settings are read-only in this mode".to_owned()))
}

pub(crate) fn api_error(err: SettingsError) -> ApiError {
    match err {
        SettingsError::Conflict { .. } | SettingsError::Locked { .. } => {
            ApiError::Conflict(err.to_string())
        }
        SettingsError::PasswordRequired | SettingsError::BadPassword => {
            ApiError::Forbidden(err.to_string())
        }
        SettingsError::TooMany(secs) => ApiError::TooMany(secs),
        SettingsError::Invalid(issues) => {
            ApiError::Invalid(issues.iter().map(IssueDto::from).collect())
        }
        SettingsError::ApplyFailed(_) => ApiError::BadRequest(err.to_string()),
        SettingsError::Unavailable(m) => ApiError::Unavailable(m),
        SettingsError::Store(e) => ApiError::from(e),
        SettingsError::Crypto(e) => ApiError::internal("settings crypto failed", e),
    }
}

/// Saves a change and restarts the hub with it.
#[utoipa::path(
    patch, path = "/api/v1/settings", operation_id = "update_settings", tag = "system",
    request_body = SettingsPatch,
    responses(
        (status = 200, description = "Saved and applied", body = SavedDto),
        (status = 400, description = "Invalid; `issues` lists each problem with its path", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong for a sensitive change", body = ErrorBody),
        (status = 409, description = "Stale revision, or a field set in hub.toml or the environment", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn update(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Authed(current): Authed,
    Json(req): Json<SettingsPatch>,
) -> Result<Json<SavedDto>, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let saved = service(&state)?
        .update(
            &current.user,
            ip,
            req.base_revision,
            &req.changes,
            req.password.as_deref(),
        )
        .await
        .map_err(api_error)?;
    Ok(Json(SavedDto {
        revision: saved.revision,
        applied: saved.applied,
    }))
}

/// The JSON schema of `hub.toml`, for field help and defaults.
#[utoipa::path(
    get, path = "/api/v1/settings/schema", operation_id = "settings_schema", tag = "system",
    responses(
        (status = 200, description = "The schema", body = Object),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn schema(_auth: Authed) -> Json<Value> {
    Json(serde_json::to_value(domovoi_config::HubConfig::json_schema()).unwrap_or(Value::Null))
}

async fn run_test(
    state: &ApiState,
    current: &crate::auth::Current,
    target: Target,
    req: &TestRequest,
) -> Result<Json<TestResultDto>, ApiError> {
    let settings = service(state)?;
    settings
        .admit_test(current.session_hash)
        .map_err(api_error)?;
    let result = settings
        .test(target, &req.changes)
        .await
        .map_err(api_error)?;
    Ok(Json(TestResultDto {
        ok: result.ok,
        message: result.message,
        latency_ms: result.latency_ms,
    }))
}

/// Posts "Domovoi test message" with candidate Discord settings.
#[utoipa::path(
    post, path = "/api/v1/settings/test/discord", operation_id = "test_discord", tag = "system",
    request_body = TestRequest,
    responses(
        (status = 200, description = "Tried", body = TestResultDto),
        (status = 400, description = "Invalid candidate", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 429, description = "More than 10 tests a minute", body = ErrorBody),
    )
)]
pub(super) async fn test_discord(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Json(req): Json<TestRequest>,
) -> Result<Json<TestResultDto>, ApiError> {
    run_test(&state, &current, Target::Discord, &req).await
}

/// One minimal call to the candidate primary model.
#[utoipa::path(
    post, path = "/api/v1/settings/test/llm", operation_id = "test_llm", tag = "system",
    request_body = TestRequest,
    responses(
        (status = 200, description = "Tried", body = TestResultDto),
        (status = 400, description = "Invalid candidate", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 429, description = "More than 10 tests a minute", body = ErrorBody),
    )
)]
pub(super) async fn test_llm(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Json(req): Json<TestRequest>,
) -> Result<Json<TestResultDto>, ApiError> {
    run_test(&state, &current, Target::Llm, &req).await
}

/// One GET of the candidate heartbeat URL.
#[utoipa::path(
    post, path = "/api/v1/settings/test/heartbeat", operation_id = "test_heartbeat", tag = "system",
    request_body = TestRequest,
    responses(
        (status = 200, description = "Tried", body = TestResultDto),
        (status = 400, description = "Invalid candidate", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 429, description = "More than 10 tests a minute", body = ErrorBody),
    )
)]
pub(super) async fn test_heartbeat(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Json(req): Json<TestRequest>,
) -> Result<Json<TestResultDto>, ApiError> {
    run_test(&state, &current, Target::Heartbeat, &req).await
}

fn dto(item: &CheckOverride) -> CheckSettingsDto {
    CheckSettingsDto {
        enabled: item.enabled,
        interval_seconds: item.interval_seconds,
        thresholds: item.thresholds.clone(),
    }
}

/// A host's checks: what its agent reports and what the hub overrides.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/checks", operation_id = "host_checks", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The checks", body = HostChecksDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn host_checks(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<HostChecksDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let checks = service(&state)?
        .host_checks(&host)
        .await
        .map_err(api_error)?;
    let mut rows: BTreeMap<String, HostCheckDto> = BTreeMap::new();
    for info in &checks.reported {
        rows.insert(
            info.key.to_string(),
            HostCheckDto {
                key: info.key.to_string(),
                reported: Some(CheckSettingsDto {
                    enabled: Some(info.enabled),
                    interval_seconds: Some(info.interval_seconds),
                    thresholds: info.thresholds.clone(),
                }),
                overrides: None,
            },
        );
    }
    for (key, item) in &checks.overrides {
        rows.entry(key.to_string())
            .or_insert_with(|| HostCheckDto {
                key: key.to_string(),
                reported: None,
                overrides: None,
            })
            .overrides = Some(dto(item));
    }
    Ok(Json(HostChecksDto {
        connected: checks.connected,
        checks: rows.into_values().collect(),
    }))
}

/// Replaces a host's check overrides and sends them to its agent; no restart.
#[utoipa::path(
    put, path = "/api/v1/hosts/{name}/checks", operation_id = "set_host_checks", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    request_body = HostChecksUpdate,
    responses(
        (status = 200, description = "Saved", body = HostChecksSaved),
        (status = 400, description = "Invalid; `issues` lists each problem", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn set_host_checks(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(name): Path<String>,
    Json(req): Json<HostChecksUpdate>,
) -> Result<Json<HostChecksSaved>, ApiError> {
    let host = host_named(&state, &name).await?;
    let mut overrides = BTreeMap::new();
    let mut issues = Vec::new();
    for (key, item) in req.overrides {
        match CheckKey::parse(&key) {
            Ok(parsed) => {
                overrides.insert(
                    parsed,
                    CheckOverride {
                        enabled: item.enabled,
                        interval_seconds: item.interval_seconds,
                        thresholds: item.thresholds,
                    },
                );
            }
            Err(err) => issues.push(IssueDto {
                path: key,
                message: err.to_string(),
                severity: "error".to_owned(),
            }),
        }
    }
    if !issues.is_empty() {
        return Err(ApiError::Invalid(issues));
    }
    let delivered = service(&state)?
        .set_host_checks(&current.user, &host, overrides)
        .await
        .map_err(api_error)?;
    Ok(Json(HostChecksSaved { delivered }))
}
