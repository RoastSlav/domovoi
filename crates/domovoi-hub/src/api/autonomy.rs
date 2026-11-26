//! The autonomy controls: read the pause switch and the suspended rules, pause, resume, reset a
//! rule's circuit breaker and resolve an unknown outcome.
//!
//! Each is human-only (a session and the CSRF header) and goes through `services.policy`. Pausing
//! needs no password because it narrows what the hub may do; the others carry the admin's
//! password in the body, checked against the client address after trusted-proxy handling, so they
//! share the login limiter. No endpoint accepts a permit or an approval from the client.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use domovoi_core::StepId;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::policy::command_error;
use super::{ApiState, Authed};
use crate::auth::proxy;
use crate::services::{AutonomyView, StepUp};

/// One rule whose circuit breaker is open.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SuspendedRuleDto {
    /// The rule id.
    pub rule: String,
    /// How many failures are counted.
    pub failures: u32,
    /// When the earliest counted failure settled.
    pub since: String,
}

/// The autonomy switches.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AutonomyDto {
    /// Whether autonomy is paused.
    pub paused: bool,
    /// Who last changed the switch.
    pub paused_by: Option<String>,
    /// When it last changed.
    pub paused_at: Option<String>,
    /// Why it was paused.
    pub reason: Option<String>,
    /// Rules whose breaker is open.
    pub suspended_rules: Vec<SuspendedRuleDto>,
}

impl From<AutonomyView> for AutonomyDto {
    fn from(view: AutonomyView) -> Self {
        Self {
            paused: view.paused,
            paused_by: view.paused_by,
            paused_at: view.paused_at.map(|at| at.to_string()),
            reason: view.reason,
            suspended_rules: view
                .suspended_rules
                .into_iter()
                .map(|rule| SuspendedRuleDto {
                    rule: rule.rule,
                    failures: rule.failures,
                    since: rule.since.to_string(),
                })
                .collect(),
        }
    }
}

/// Pause autonomy.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PauseRequest {
    /// Why, 1 to 280 characters.
    pub reason: String,
}

/// A command that needs the admin's password.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PasswordRequest {
    /// The admin's password.
    #[serde(default)]
    pub password: Option<String>,
}

/// Resolve an unknown outcome.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ResolveUnknownRequest {
    /// What the owner checked, 1 to 280 characters.
    pub note: String,
    /// The admin's password.
    #[serde(default)]
    pub password: Option<String>,
}

fn parse_step(text: &str) -> Result<StepId, ApiError> {
    text.parse().map_err(|_| ApiError::NotFound)
}

/// The pause switch and the suspended rules.
#[utoipa::path(
    get, path = "/api/v1/policy/autonomy", operation_id = "autonomy_state", tag = "policy",
    responses(
        (status = 200, description = "The switches", body = AutonomyDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 503, description = "Unattended execution is not set up", body = ErrorBody),
    )
)]
pub(super) async fn state(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<AutonomyDto>, ApiError> {
    let view = state
        .services()
        .policy
        .autonomy()
        .await
        .map_err(command_error)?;
    Ok(Json(view.into()))
}

/// Pauses autonomy: no new permit and no dispatch of one already issued.
#[utoipa::path(
    post, path = "/api/v1/policy/autonomy/pause", operation_id = "autonomy_pause", tag = "policy",
    request_body = PauseRequest,
    responses(
        (status = 200, description = "Paused, or already paused", body = AutonomyDto),
        (status = 400, description = "The reason is empty or too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 503, description = "Unattended execution is not set up", body = ErrorBody),
    )
)]
pub(super) async fn pause(
    State(state): State<ApiState>,
    auth: Authed,
    Json(req): Json<PauseRequest>,
) -> Result<Json<AutonomyDto>, ApiError> {
    let policy = &state.services().policy;
    policy
        .pause_autonomy(&auth.caller(), &req.reason)
        .await
        .map_err(command_error)?;
    Ok(Json(policy.autonomy().await.map_err(command_error)?.into()))
}

/// Resumes autonomy. Needs the admin's password.
#[utoipa::path(
    post, path = "/api/v1/policy/autonomy/resume", operation_id = "autonomy_resume", tag = "policy",
    request_body = PasswordRequest,
    responses(
        (status = 200, description = "Resumed, or not paused", body = AutonomyDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
        (status = 503, description = "Unattended execution is not set up", body = ErrorBody),
    )
)]
pub(super) async fn resume(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    body: Option<Json<PasswordRequest>>,
) -> Result<Json<AutonomyDto>, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    // No body at all is "no password": the answer is 403 asking for it, not a parse error.
    let password = body.and_then(|Json(body)| body.password);
    let policy = &state.services().policy;
    policy
        .resume_autonomy(
            &auth.caller(),
            StepUp {
                ip,
                password: password.as_deref(),
            },
        )
        .await
        .map_err(command_error)?;
    Ok(Json(policy.autonomy().await.map_err(command_error)?.into()))
}

/// Resets one rule's circuit breaker. Needs the admin's password.
#[utoipa::path(
    post, path = "/api/v1/policy/rules/{rule}/reset", operation_id = "policy_rule_reset", tag = "policy",
    params(("rule" = String, Path, description = "Rule id")),
    request_body = PasswordRequest,
    responses(
        (status = 200, description = "Reset", body = AutonomyDto),
        (status = 400, description = "Not a rule id", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
        (status = 503, description = "Unattended execution is not set up", body = ErrorBody),
    )
)]
pub(super) async fn reset_rule(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path(rule): Path<String>,
    body: Option<Json<PasswordRequest>>,
) -> Result<Json<AutonomyDto>, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let password = body.and_then(|Json(body)| body.password);
    let policy = &state.services().policy;
    policy
        .reset_rule(
            &auth.caller(),
            &rule,
            StepUp {
                ip,
                password: password.as_deref(),
            },
        )
        .await
        .map_err(command_error)?;
    Ok(Json(policy.autonomy().await.map_err(command_error)?.into()))
}

/// Clears the unattended block of a step whose outcome is unknown. Needs the admin's password.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/steps/{step}/resolve-unknown",
    operation_id = "resolve_unknown_outcome", tag = "operations",
    params(
        ("id" = String, Path, description = "Operation id"),
        ("step" = String, Path, description = "Execution step id"),
    ),
    request_body = ResolveUnknownRequest,
    responses(
        (status = 204, description = "Resolved"),
        (status = 400, description = "The note is empty or too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong", body = ErrorBody),
        (status = 404, description = "No such step", body = ErrorBody),
        (status = 409, description = "The step has no unresolved unknown outcome", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
        (status = 503, description = "Unattended execution is not set up", body = ErrorBody),
    )
)]
pub(super) async fn resolve_unknown(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path((_operation, step)): Path<(String, String)>,
    Json(req): Json<ResolveUnknownRequest>,
) -> Result<StatusCode, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let step = parse_step(&step)?;
    state
        .services()
        .policy
        .resolve_unknown(
            &auth.caller(),
            step,
            &req.note,
            StepUp {
                ip,
                password: req.password.as_deref(),
            },
        )
        .await
        .map_err(command_error)?;
    Ok(StatusCode::NO_CONTENT)
}
