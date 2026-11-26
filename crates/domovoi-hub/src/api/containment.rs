//! Containment over HTTP: what a host's blocks look like, the owner's refresh, and the early
//! release of a block. Approving a block is the operation step approval, extended in
//! `operations.rs` (ADR 0038).

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use domovoi_core::capability::containment::BLOCK_SOURCE_ID;
use domovoi_core::containment::parse_message;
use domovoi_core::ports::CapabilityRegistry;
use domovoi_core::{ContractId, OperationId, StepId};

use super::dto_containment::{BlockDto, ContainmentStatusDto, ReleaseRequest};
use super::error::{ApiError, ErrorBody};
use super::operation_errors::operation_error;
use super::{ApiState, Authed, host_named};
use crate::auth::proxy;
use crate::services::{ContainmentCommandError, StepUp};

fn coded(status: StatusCode, code: &'static str, message: String) -> ApiError {
    ApiError::Coded {
        status,
        code,
        message,
    }
}

/// The answer to a refused containment command: each refusal the helper made keeps its stable
/// token as the code.
pub(crate) fn containment_error(err: ContainmentCommandError) -> ApiError {
    let message = err.to_string();
    match err {
        ContainmentCommandError::Disabled => ApiError::Unavailable(message),
        ContainmentCommandError::WebOnly
        | ContainmentCommandError::PasswordRequired
        | ContainmentCommandError::BadPassword
        | ContainmentCommandError::TypedSourceMismatch => ApiError::Forbidden(message),
        ContainmentCommandError::TooMany { retry_after_secs } => {
            ApiError::TooMany(retry_after_secs)
        }
        ContainmentCommandError::NotFound | ContainmentCommandError::NotContainment => {
            ApiError::NotFound
        }
        ContainmentCommandError::UnverifiedNotAcknowledged => {
            coded(StatusCode::CONFLICT, "unverified_not_acknowledged", message)
        }
        ContainmentCommandError::ApprovalWindowTooShort => {
            coded(StatusCode::CONFLICT, "approval_window_too_short", message)
        }
        ContainmentCommandError::NotConnected | ContainmentCommandError::Unknown => {
            ApiError::Conflict(message)
        }
        ContainmentCommandError::Refused(text) => match parse_message(&text) {
            Some((token, _)) => coded(StatusCode::CONFLICT, token.as_str(), text),
            None => ApiError::Conflict(text),
        },
        ContainmentCommandError::NotReleasable(_) | ContainmentCommandError::Conflict(_) => {
            ApiError::Conflict(message)
        }
        ContainmentCommandError::BadRequest(text) => ApiError::BadRequest(text),
        ContainmentCommandError::Operation(err) => operation_error(err),
        ContainmentCommandError::Store(err) => ApiError::from(err),
    }
}

async fn status_of(
    state: &ApiState,
    host: &domovoi_core::Host,
    view: &crate::services::ContainmentHostView,
) -> ContainmentStatusDto {
    let available = match ContractId::parse(BLOCK_SOURCE_ID) {
        Ok(id) => state
            .deps
            .capabilities
            .availability(host.id, &id)
            .await
            .is_available(),
        Err(_) => false,
    };
    ContainmentStatusDto::of(view, available)
}

/// What a host's blocks look like. Display only, and answered when containment is not available.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/containment", operation_id = "get_host_containment", tag = "containment",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The host's blocks and what the helper last said", body = ContainmentStatusDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn status(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<ContainmentStatusDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let view = state
        .services()
        .containment
        .host_view(host.id)
        .await
        .map_err(containment_error)?;
    Ok(Json(status_of(&state, &host, &view).await))
}

/// Asks the host's helper for its blocks, gate and drills. Owner-initiated only: nothing polls.
#[utoipa::path(
    post, path = "/api/v1/hosts/{name}/containment/refresh", operation_id = "refresh_host_containment", tag = "containment",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "What the helper said", body = ContainmentStatusDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Chat cannot refresh", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 409, description = "The agent is not connected, or the helper refused", body = ErrorBody),
    )
)]
pub(super) async fn refresh(
    State(state): State<ApiState>,
    auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<ContainmentStatusDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let view = state
        .services()
        .containment
        .refresh(&auth.caller(), host.id)
        .await
        .map_err(containment_error)?;
    Ok(Json(status_of(&state, &host, &view).await))
}

/// Releases a block early: asks for the password again and the source typed out, then removes
/// the block under the plan hash of its approval.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/steps/{step}/containment/release", operation_id = "release_containment_block", tag = "containment",
    params(
        ("id" = String, Path, description = "Operation id"),
        ("step" = String, Path, description = "Execution step id"),
    ),
    request_body = ReleaseRequest,
    responses(
        (status = 200, description = "Released", body = BlockDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "The password is missing or wrong, or the typed source is not the block's", body = ErrorBody),
        (status = 404, description = "No such operation, step or block", body = ErrorBody),
        (status = 409, description = "The block cannot be released now, or the agent is not connected", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn release(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path((id, step)): Path<(String, String)>,
    Json(req): Json<ReleaseRequest>,
) -> Result<Json<BlockDto>, ApiError> {
    let (id, step): (OperationId, StepId) = (
        id.parse().map_err(|_| ApiError::NotFound)?,
        step.parse().map_err(|_| ApiError::NotFound)?,
    );
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let proof = StepUp {
        ip,
        password: req.password.as_deref(),
    };
    let block = state
        .services()
        .containment
        .release(
            &auth.caller(),
            id,
            step,
            req.confirm_source.as_deref(),
            proof,
        )
        .await
        .map_err(containment_error)?;
    Ok(Json(BlockDto::from(&block)))
}
