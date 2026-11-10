//! The latest release check and the commands that update a hub, its agents or containers.

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::version::HUB_VERSION;
use super::{ApiState, Authed};
use crate::runtime::updates::{CheckError, UpdateInfo};
use domovoi_proto::is_release_tag;

const INSTALL: &str = "https://github.com/RoastSlav/domovoi/releases/download";
/// The environment variable the Compose files read to pin the image they pull, so a
/// container deployment gets the release the banner names and not whatever is `latest`.
const IMAGE_TAG: &str = "DOMOVOI_IMAGE_TAG";

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
    /// Replaces the hub's binary in place and restarts its service. `None` when the hub
    /// runs in a container, where the image is what changes.
    pub upgrade_command: Option<String>,
    /// Reinstalls the hub and restarts its service.
    pub hub_command: Option<String>,
    /// Reinstalls an agent and restarts its service.
    pub agent_command: Option<String>,
    /// Pulls and recreates a container deployment.
    pub docker_command: Option<String>,
}

impl UpdateDto {
    /// The API's view of `info`, measured against the version this hub runs now.
    /// A check whose tag is not a plain release tag (`is_release_tag`) is dropped whole,
    /// because the tag is shown and put into commands: a hub that saved an unsafe one,
    /// as an older build could, then offers nothing rather than something to be run.
    fn new(info: Option<&UpdateInfo>) -> Self {
        let info = if info.is_some_and(|info| is_release_tag(&info.latest)) {
            info
        } else {
            None
        };
        let latest = info.map(|info| info.latest.clone());
        let newer = latest
            .as_deref()
            .is_some_and(|latest| super::version::is_older(HUB_VERSION, latest));
        let commands = latest.as_deref().map(commands);
        let upgrade = latest.as_deref().filter(|_| !in_container()).map(|tag| {
            format!("sudo domovoi upgrade --version {tag} && sudo systemctl restart domovoi-hub")
        });
        Self {
            current: HUB_VERSION.to_owned(),
            latest,
            newer,
            url: info.map(|info| info.url.clone()),
            notes: info.and_then(|info| info.notes.clone()),
            published_at: info.and_then(|info| info.published_at.clone()),
            checked_at: info.map(|info| info.checked_at.to_string()),
            upgrade_command: upgrade,
            hub_command: commands.as_ref().map(|commands| commands.0.clone()),
            agent_command: commands.as_ref().map(|commands| commands.1.clone()),
            docker_command: commands.map(|commands| commands.2),
        }
    }
}

/// Whether this process runs in a container, which Docker and Podman mark with a file.
fn in_container() -> bool {
    std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists()
}

/// The commands that bring a deployment to `tag`: hub, one agent, containers. The tag has
/// passed [`is_release_tag`] by the time this runs, which is what lets it be written into
/// a URL path and onto a shell command line as it is: no character left in a vetted tag is
/// special to a shell or to a URL path, so no quoting or escaping is needed. The image is
/// tagged with the same version without its leading `v`, which is how the release
/// workflow pushes it.
fn commands(tag: &str) -> (String, String, String) {
    let hub = format!(
        "curl -fsSL {INSTALL}/{tag}/install.sh | sudo sh -s -- --role hub --version {tag} && sudo systemctl restart domovoi-hub"
    );
    let agent = format!(
        "curl -fsSL {INSTALL}/{tag}/install.sh | sudo sh -s -- --role agent --version {tag} && sudo systemctl restart domovoi-agent"
    );
    // The image carries the release's own tag, the version without the tag's leading `v`.
    let image = tag.strip_prefix('v').unwrap_or(tag);
    let docker = format!(
        "{IMAGE_TAG}={image} docker compose -f hub.yml pull && {IMAGE_TAG}={image} docker compose -f hub.yml up -d"
    );
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
