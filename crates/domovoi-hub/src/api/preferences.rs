//! The signed-in person's own display preferences: how much detail the pages show them.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed};
use crate::services::{
    PreferenceError, Preferences, PreferencesUpdate, PresentationInput, PresentationPrefs,
};

/// A person's stored preferences. A person who never saved any gets the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PreferencesDto {
    /// The layout of the stored document; currently 1.
    pub version: u32,
    /// When the preferences were last saved, as RFC 3339; null while none are saved.
    pub updated_at: Option<String>,
    /// How much detail pages show.
    pub presentation: PresentationDto,
}

/// How much detail pages show: one level for every page, and levels for single pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PresentationDto {
    /// The level pages use unless they have their own: `detailed`, `standard` or `essential`.
    pub profile: String,
    /// Levels for single pages, by page key (a lowercase letter, then lowercase letters,
    /// digits, `-` and `_`, up to 32 characters; at most 32 pages). Each value is
    /// `detailed`, `standard` or `essential`.
    #[serde(default)]
    pub pages: BTreeMap<String, String>,
}

/// A change to the preferences. A group left out or null stays as it is; a group given
/// replaces the stored one as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, ToSchema)]
pub struct PreferencesUpdateDto {
    /// The new presentation settings.
    #[serde(default)]
    pub presentation: Option<PresentationDto>,
}

impl From<PresentationPrefs> for PresentationDto {
    fn from(prefs: PresentationPrefs) -> Self {
        Self {
            profile: prefs.profile.name().to_owned(),
            pages: prefs
                .pages
                .into_iter()
                .map(|(key, level)| (key, level.name().to_owned()))
                .collect(),
        }
    }
}

impl From<Preferences> for PreferencesDto {
    fn from(prefs: Preferences) -> Self {
        Self {
            version: crate::services::PREFERENCES_VERSION,
            updated_at: prefs.updated_at.map(|at| at.to_string()),
            presentation: prefs.presentation.into(),
        }
    }
}

impl From<PreferencesUpdateDto> for PreferencesUpdate {
    fn from(dto: PreferencesUpdateDto) -> Self {
        Self {
            presentation: dto.presentation.map(|presentation| PresentationInput {
                profile: presentation.profile,
                pages: presentation.pages,
            }),
        }
    }
}

fn error_of(err: PreferenceError) -> ApiError {
    match err {
        PreferenceError::Invalid(text) => ApiError::BadRequest(text),
        PreferenceError::Store(err) => ApiError::internal("saving preferences", err),
    }
}

/// The signed-in person's display preferences.
#[utoipa::path(
    get, path = "/api/v1/me/preferences", operation_id = "get_preferences", tag = "me",
    responses(
        (status = 200, description = "The preferences, or the defaults", body = PreferencesDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn get(
    State(state): State<ApiState>,
    auth: Authed,
) -> Result<Json<PreferencesDto>, ApiError> {
    let caller = auth.caller();
    let prefs = state
        .services()
        .preferences
        .get(&caller)
        .await
        .map_err(error_of)?;
    Ok(Json(prefs.into()))
}

/// Saves the signed-in person's display preferences.
#[utoipa::path(
    put, path = "/api/v1/me/preferences", operation_id = "update_preferences", tag = "me",
    request_body = PreferencesUpdateDto,
    responses(
        (status = 200, description = "The preferences after the change", body = PreferencesDto),
        (status = 400, description = "Unknown level or malformed page key", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn put(
    State(state): State<ApiState>,
    auth: Authed,
    Json(body): Json<PreferencesUpdateDto>,
) -> Result<Json<PreferencesDto>, ApiError> {
    let caller = auth.caller();
    let prefs = state
        .services()
        .preferences
        .update(&caller, body.into())
        .await
        .map_err(error_of)?;
    Ok(Json(prefs.into()))
}
