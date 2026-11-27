//! `GET /api/v1/session` and `POST /api/v1/auth/{login,logout,setup}`.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::audit_note::note;
use super::dto_admin::{LoginRequest, SessionDto, SetupRequest, UserDto};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed};
use crate::auth::{AuthError, NewSession, csrf, proxy, session};

fn session_dto(user: &domovoi_core::User, csrf_token: String) -> SessionDto {
    SessionDto {
        user: UserDto {
            id: user.id.to_string(),
            username: user.username.clone(),
        },
        csrf_token,
    }
}

fn with_cookie(state: &ApiState, new: NewSession) -> Response {
    let cookie = session::set_cookie(&new.id, state.auth.web().https());
    let mut response = Json(session_dto(&new.user, new.csrf_token)).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

/// The current session and its CSRF token.
#[utoipa::path(
    get, path = "/api/v1/session", operation_id = "get_session", tag = "auth",
    responses(
        (status = 200, description = "Logged in", body = SessionDto),
        (status = 401, description = "Not logged in; the error is `setup_required` while no account exists", body = ErrorBody),
    )
)]
pub(super) async fn session(Authed(current): Authed) -> Json<SessionDto> {
    Json(session_dto(&current.user, current.csrf_token))
}

/// Logs in and sets the session cookie. Five failures a minute from one address lock it
/// out for a while.
#[utoipa::path(
    post, path = "/api/v1/auth/login", operation_id = "login", tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Logged in; the cookie is set", body = SessionDto),
        (status = 401, description = "Wrong username or password", body = ErrorBody),
        (status = 429, description = "Too many attempts", body = ErrorBody),
    )
)]
pub(super) async fn login(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    match state.auth.login(ip, &req.username, &req.password).await {
        Ok(new) => {
            note(
                &state,
                format!("user:{}", new.user.id),
                "auth.login",
                format!("user:{}", new.user.id),
                json!({ "ip": ip.to_string() }),
            )
            .await;
            Ok(with_cookie(&state, new))
        }
        // The disk, not the caller, refused: that is not a failed login, so no audit row (it
        // could not be written anyway) and no count against the user.
        Err(err @ (AuthError::Store(_) | AuthError::Unwritable(_))) => Err(err.into()),
        Err(err) => {
            // The name is what the caller typed: cut it, and never record the password.
            let name: String = req.username.chars().take(64).collect();
            note(
                &state,
                format!("web:{ip}"),
                "auth.login_failed",
                format!("username:{name}"),
                json!({ "ip": ip.to_string(), "reason": err.to_string() }),
            )
            .await;
            Err(err.into())
        }
    }
}

/// Ends the session and clears the cookie.
#[utoipa::path(
    post, path = "/api/v1/auth/logout", operation_id = "logout", tag = "auth",
    responses((status = 204, description = "Logged out"))
)]
pub(super) async fn logout(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(id) = csrf::session_id(&headers) {
        let who = state.auth.current(id).await?;
        state.auth.logout(id).await?;
        if let Some(current) = who {
            note(
                &state,
                format!("user:{}", current.user.id),
                "auth.logout",
                format!("user:{}", current.user.id),
                json!({}),
            )
            .await;
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&session::clear_cookie(state.auth.web().https())) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    Ok(response)
}

/// Creates the admin account with the one-time token from the setup link, and logs in.
/// Only works before the first user exists.
#[utoipa::path(
    post, path = "/api/v1/auth/setup", operation_id = "setup", tag = "auth",
    request_body = SetupRequest,
    responses(
        (status = 200, description = "Account created and logged in", body = SessionDto),
        (status = 400, description = "Invalid username or password", body = ErrorBody),
        (status = 403, description = "Wrong, used or expired token", body = ErrorBody),
        (status = 409, description = "Setup is closed", body = ErrorBody),
    )
)]
pub(super) async fn setup(
    State(state): State<ApiState>,
    Json(req): Json<SetupRequest>,
) -> Result<Response, ApiError> {
    let new = state
        .auth
        .setup(&req.token, &req.username, &req.password)
        .await?;
    note(
        &state,
        format!("user:{}", new.user.id),
        "auth.setup",
        format!("user:{}", new.user.id),
        json!({ "username": new.user.username }),
    )
    .await;
    Ok(with_cookie(&state, new))
}
