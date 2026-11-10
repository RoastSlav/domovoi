//! The latest release check and the commands that update a hub, its agents or containers.

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::version::HUB_VERSION;
use super::{ApiState, Authed};
use crate::runtime::updates::{CheckError, UpdateInfo};

const INSTALL: &str = "https://github.com/RoastSlav/domovoi/releases/download";

/// The newest release this hub knows about, with commands ready to copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct UpdateDto {
    /// The version this hub runs.
    pub current: String,
    /// The release's tag, for example `v0.5.0`; `None` until the first check succeeds.
    pub latest: Option<String>,
    /// Whether `latest` is newer than `current`.
    pub newer: bool,
    /// The release's page on GitHub.
    pub url: Option<String>,
    /// The release notes, cut to 4000 characters.
    pub notes: Option<String>,
    /// When the release was published.
    pub published_at: Option<String>,
    /// When this hub last checked.
    pub checked_at: Option<String>,
    /// Reinstalls the hub and restarts its service.
    pub hub_command: Option<String>,
    /// Reinstalls an agent and restarts its service.
    pub agent_command: Option<String>,
    /// Pulls and recreates a container deployment.
    pub docker_command: Option<String>,
}

impl UpdateDto {
    /// The API's view of `info`, measured against the version this hub runs now.
    fn new(info: Option<&UpdateInfo>) -> Self {
        let latest = info.map(|info| info.latest.clone());
        let newer = latest
            .as_deref()
            .is_some_and(|latest| super::version::is_older(HUB_VERSION, latest));
        let commands = latest.as_deref().map(commands);
        Self {
            current: HUB_VERSION.to_owned(),
            latest,
            newer,
            url: info.map(|info| info.url.clone()),
            notes: info.and_then(|info| info.notes.clone()),
            published_at: info.and_then(|info| info.published_at.clone()),
            checked_at: info.map(|info| info.checked_at.to_string()),
            hub_command: commands.as_ref().map(|commands| commands.0.clone()),
            agent_command: commands.as_ref().map(|commands| commands.1.clone()),
            docker_command: commands.map(|commands| commands.2),
        }
    }
}

/// The commands that bring a deployment to `tag`: hub, one agent, containers.
fn commands(tag: &str) -> (String, String, String) {
    let hub = format!(
        "curl -fsSL {INSTALL}/{tag}/install.sh | sudo sh -s -- --role hub --version {tag} && sudo systemctl restart domovoi-hub"
    );
    let agent = format!(
        "curl -fsSL {INSTALL}/{tag}/install.sh | sudo sh -s -- --role agent --version {tag} && sudo systemctl restart domovoi-agent"
    );
    let docker = "docker compose -f hub.yml pull && docker compose -f hub.yml up -d".to_owned();
    (hub, agent, docker)
}

/// The last release check, if the hub has made one.
#[utoipa::path(
    get, path = "/api/v1/system/update", operation_id = "get_update", tag = "system",
    responses(
        (status = 200, description = "The last check and how to update", body = UpdateDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn show(State(state): State<ApiState>, _auth: Authed) -> Json<UpdateDto> {
    let stored = state.deps.update.stored();
    Json(UpdateDto::new(stored.as_ref()))
}

/// Checks for a release now, at most once a minute.
#[utoipa::path(
    post, path = "/api/v1/system/update/check", operation_id = "check_update", tag = "system",
    responses(
        (status = 200, description = "The new check", body = UpdateDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 429, description = "Checked less than a minute ago", body = ErrorBody),
        (status = 503, description = "GitHub did not answer with a release", body = ErrorBody),
    )
)]
pub(super) async fn check(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<UpdateDto>, ApiError> {
    match state.deps.update.check_now().await {
        Ok(info) => Ok(Json(UpdateDto::new(Some(&info)))),
        Err(CheckError::TooSoon(seconds)) => Err(ApiError::TooMany(seconds)),
        Err(CheckError::Failed(message)) => Err(ApiError::Unavailable(format!(
            "could not check for updates: {message}"
        ))),
    }
}
