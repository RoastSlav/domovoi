//! The profile of a host: what its machine looks like while it behaves normally, and
//! asking its agent for a new one now.
//!
//! The text shown here is the note the hub wrote, so triage and the person reading the
//! profile see the same thing. A host with no profile yet answers 404: the profile task
//! collects one on its next tick, and the UI says so rather than showing an empty page.

use domovoi_core::Host;

use axum::Json;
use axum::extract::{Path, State};

use super::dto_admin::{ProfileDto, ProfileSectionDto};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};
use crate::profile::{HostProfile, ProfileDeps, ProfileError, note_text, stored};

/// This hub's profile task; 503 when it was built without one.
fn profiles(state: &ApiState) -> Result<ProfileDeps, ApiError> {
    state
        .deps
        .profiles
        .clone()
        .ok_or_else(|| ApiError::Unavailable("the hub does not collect profiles".to_owned()))
}

/// How a profile failure answers the request.
fn profile_error(err: ProfileError) -> ApiError {
    match err {
        ProfileError::NotConnected => ApiError::Conflict("the agent is not connected".to_owned()),
        ProfileError::Store(store) => ApiError::from(store),
    }
}

fn profile_dto(host: &Host, profile: &HostProfile) -> ProfileDto {
    ProfileDto {
        collected_at: profile.collected_at.to_string(),
        note: profile.note.map(|id| id.to_string()),
        markdown: note_text(host, profile),
        sections: profile
            .sections
            .iter()
            .map(|section| ProfileSectionDto {
                title: section.title.clone(),
                ok: section.ok,
            })
            .collect(),
    }
}

/// What `host`'s machine looks like while it behaves normally; 404 before the hub has
/// collected one.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/profile", operation_id = "get_host_profile", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The profile", body = ProfileDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host, or no profile yet", body = ErrorBody),
        (status = 503, description = "The hub does not collect profiles", body = ErrorBody),
    )
)]
pub(super) async fn get(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<ProfileDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let deps = profiles(&state)?;
    match stored(&deps, host.id).await.map_err(profile_error)? {
        None => Err(ApiError::NotFound),
        Some(profile) => Ok(Json(profile_dto(&host, &profile))),
    }
}

/// Collects the profile of `host` again now, and answers with it. 409 when its agent is
/// not connected: there is nobody to ask.
#[utoipa::path(
    post, path = "/api/v1/hosts/{name}/profile", operation_id = "refresh_host_profile", tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The profile just collected", body = ProfileDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 409, description = "The host has no connected agent", body = ErrorBody),
        (status = 503, description = "The hub does not collect profiles", body = ErrorBody),
    )
)]
pub(super) async fn refresh(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<ProfileDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let deps = profiles(&state)?;
    let profile = crate::profile::refresh(&deps, &host)
        .await
        .map_err(profile_error)?;
    Ok(Json(profile_dto(&host, &profile)))
}
