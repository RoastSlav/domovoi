//! The helper endpoints: what the hub knows about a host's helper, asking it again, and the
//! owner's pin of its key. The hub computes every value shown; the pin needs the password and
//! the full fingerprint typed out, and is made by the helper trust service, never here.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};

use super::dto_helper::{HelperDto, PinKeyRequest, UnpinKeyRequest};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};
use crate::auth::proxy;
use crate::services::{HelperError, StepUp};

fn coded(status: StatusCode, code: &'static str, message: String) -> ApiError {
    ApiError::Coded {
        status,
        code,
        message,
    }
}

fn api_error(err: HelperError) -> ApiError {
    let message = err.to_string();
    match err {
        HelperError::NoSuchHost => ApiError::NotFound,
        HelperError::WebOnly => ApiError::Forbidden(message),
        HelperError::NoHelperKey => coded(StatusCode::CONFLICT, "no_helper_key", message),
        HelperError::FingerprintMismatch => coded(
            StatusCode::UNPROCESSABLE_ENTITY,
            "fingerprint_mismatch",
            message,
        ),
        HelperError::PasswordRequired => {
            coded(StatusCode::UNAUTHORIZED, "password_required", message)
        }
        HelperError::BadPassword => coded(StatusCode::UNAUTHORIZED, "wrong_password", message),
        HelperError::TooMany { retry_after_secs } => ApiError::TooMany(retry_after_secs),
        HelperError::Offline => ApiError::Conflict(message),
        HelperError::RecheckTooSoon { .. } => {
            coded(StatusCode::TOO_MANY_REQUESTS, "recheck_too_soon", message)
        }
        HelperError::Store(err) => ApiError::from(err),
    }
}

/// What the hub knows about host `name`'s helper.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/helper", operation_id = "get_host_helper", tag = "helper",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The helper's key and attestation state", body = HelperDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn show(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<HelperDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let view = state
        .services()
        .helper_trust
        .view(host.name)
        .await
        .map_err(api_error)?;
    Ok(Json(HelperDto::from(&view)))
}

/// Asks host `name`'s helper for a fresh attestation now.
#[utoipa::path(
    post, path = "/api/v1/hosts/{name}/helper/recheck", operation_id = "recheck_host_helper", tag = "helper",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 202, description = "Asked; read the helper again for the outcome"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 409, description = "The host is not connected", body = ErrorBody),
        (status = 429, description = "The helper was asked less than ten seconds ago (recheck_too_soon)", body = ErrorBody),
    )
)]
pub(super) async fn recheck(
    State(state): State<ApiState>,
    auth: Authed,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    let host = host_named(&state, &name).await?;
    state
        .services()
        .helper_trust
        .recheck(&auth.caller(), host.name)
        .await
        .map_err(api_error)?;
    Ok(StatusCode::ACCEPTED)
}

/// Pins the key the hub holds from host `name`'s helper. Asks for the password again and for
/// the key's full fingerprint, typed from `domovoi helper fingerprint` on the host.
#[utoipa::path(
    put, path = "/api/v1/hosts/{name}/helper/key", operation_id = "pin_host_helper_key", tag = "helper",
    params(("name" = String, Path, description = "Host name")),
    request_body = PinKeyRequest,
    responses(
        (status = 200, description = "Pinned", body = HelperDto),
        (status = 401, description = "Not logged in, or the password is missing or wrong (wrong_password)", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 409, description = "The hub holds no key from this host's helper (no_helper_key)", body = ErrorBody),
        (status = 422, description = "The typed text is not the full fingerprint of the held key (fingerprint_mismatch)", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn pin(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path(name): Path<String>,
    Json(req): Json<PinKeyRequest>,
) -> Result<Json<HelperDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let proof = StepUp {
        ip: proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies),
        password: req.password.as_deref(),
    };
    let view = state
        .services()
        .helper_trust
        .pin(&auth.caller(), host.name, &req.fingerprint, proof)
        .await
        .map_err(api_error)?;
    Ok(Json(HelperDto::from(&view)))
}

/// Removes the pin of host `name`'s helper key. Containment is blocked until it is pinned
/// again. Asks for the password again.
#[utoipa::path(
    delete, path = "/api/v1/hosts/{name}/helper/key", operation_id = "unpin_host_helper_key", tag = "helper",
    params(("name" = String, Path, description = "Host name")),
    request_body = UnpinKeyRequest,
    responses(
        (status = 200, description = "Unpinned", body = HelperDto),
        (status = 401, description = "Not logged in, or the password is missing or wrong (wrong_password)", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn unpin(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path(name): Path<String>,
    Json(req): Json<UnpinKeyRequest>,
) -> Result<Json<HelperDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let proof = StepUp {
        ip: proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies),
        password: req.password.as_deref(),
    };
    let view = state
        .services()
        .helper_trust
        .unpin(&auth.caller(), host.name, proof)
        .await
        .map_err(api_error)?;
    Ok(Json(HelperDto::from(&view)))
}
