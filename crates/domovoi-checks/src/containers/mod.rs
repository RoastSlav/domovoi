//! Container checks: expected containers, health, restart loops and new containers.
//!
//! All four read containers through [`ContainerSource`], so tests use a fake and the agent
//! uses [`DockerSource`]. Targets are container names; counts and states are evidence.
//!
//! - **Managed** containers carry the `com.docker.compose.project` label (compose and
//!   Portainer stacks). Only managed containers without auto-remove are learned, so a
//!   throwaway `docker run --rm` never becomes expected.
//! - Names in [`ContainerPolicy::ignore`] are skipped by every container check.

mod docker;
mod expected;
mod health;
mod new;
mod restarts;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use jiff::{SignedDuration, Timestamp};

use crate::framework::CheckError;

pub use docker::DockerSource;
pub use expected::{EVENTS_STATE, EventLog, ExpectedContainers};
pub use health::UnhealthyContainers;
pub use new::NewContainers;
pub use restarts::{RESTART_WINDOW, RestartLoop};

/// The label that marks a container as part of a compose or Portainer stack.
pub const MANAGED_LABEL: &str = "com.docker.compose.project";
/// Most containers one listing returns.
pub const MAX_CONTAINERS: usize = 500;

/// Why the container runtime could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ContainerError(pub String);

impl From<ContainerError> for CheckError {
    fn from(err: ContainerError) -> Self {
        CheckError::Container(err.0)
    }
}

/// A container's run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// Running.
    Running,
    /// Restarting after an exit.
    Restarting,
    /// Paused.
    Paused,
    /// Created but never started.
    Created,
    /// Stopped.
    Exited,
    /// Dead or being removed.
    Dead,
}

impl RunState {
    /// The state as Docker names it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Restarting => "restarting",
            Self::Paused => "paused",
            Self::Created => "created",
            Self::Exited => "exited",
            Self::Dead => "dead",
        }
    }
}

/// A container's health check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Still in the start period.
    Starting,
    /// Passing.
    Healthy,
    /// Failing.
    Unhealthy,
}

/// One container as the checks see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    /// The name without Docker's leading `/`.
    pub name: String,
    /// The image reference it was created from.
    pub image: String,
    /// When that image was built.
    pub image_created: Option<Timestamp>,
    /// Its run state.
    pub state: RunState,
    /// Its health check result, when it has a health check.
    pub health: Option<Health>,
    /// How often Docker has restarted it.
    pub restart_count: u64,
    /// Its labels.
    pub labels: BTreeMap<String, String>,
    /// Whether Docker removes it when it exits (`--rm`).
    pub auto_remove: bool,
    /// When it last started.
    pub started_at: Option<Timestamp>,
}

impl ContainerInfo {
    /// Whether it belongs to a compose or Portainer stack.
    #[must_use]
    pub fn managed(&self) -> bool {
        self.labels.contains_key(MANAGED_LABEL)
    }

    /// Whether it may be learned into the baseline.
    #[must_use]
    pub fn learnable(&self) -> bool {
        self.managed() && !self.auto_remove
    }
}

/// One container lifecycle event, such as `die` or `start`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContainerEvent {
    /// When it happened.
    pub at: Timestamp,
    /// The container name.
    pub name: String,
    /// What happened, such as `die`, `start` or `health_status: unhealthy`.
    pub action: String,
    /// The container's image, when Docker reports it.
    pub image: Option<String>,
}

/// Where container checks read containers from.
#[async_trait]
pub trait ContainerSource: Send + Sync {
    /// Every container, running or not, at most [`MAX_CONTAINERS`].
    async fn list(&self) -> Result<Vec<ContainerInfo>, ContainerError>;
    /// Container events in `[since, until]`.
    async fn events_since(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<ContainerEvent>, ContainerError>;
    /// The inspect document of the container called `name`, with environment values removed
    /// (see [`strip_env`]).
    async fn inspect(&self, name: &str) -> Result<serde_json::Value, ContainerError>;
    /// The last `lines` lines of the container's stdout and stderr, at most
    /// [`MAX_LOG_BYTES`].
    async fn logs(&self, name: &str, lines: u16) -> Result<String, ContainerError>;
}

/// Most log bytes one [`ContainerSource::logs`] call returns.
pub const MAX_LOG_BYTES: usize = 256 * 1024;

/// Replaces every `Config.Env` entry with its variable name alone, since values often hold
/// secrets.
pub fn strip_env(inspect: &mut serde_json::Value) {
    let Some(env) = inspect
        .get_mut("Config")
        .and_then(|c| c.get_mut("Env"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for entry in env.iter_mut() {
        let name = entry
            .as_str()
            .map(|e| e.split('=').next().unwrap_or_default().to_owned())
            .unwrap_or_default();
        *entry = serde_json::Value::String(name);
    }
}

/// How container checks treat particular containers, from `[containers]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerPolicy {
    /// Names whose absence is critical rather than a warning.
    pub critical: BTreeSet<String>,
    /// Names no container check looks at.
    pub ignore: BTreeSet<String>,
    /// How long an expected container may be gone before it leaves the baseline.
    pub baseline_expiry: SignedDuration,
}

impl Default for ContainerPolicy {
    fn default() -> Self {
        Self {
            critical: BTreeSet::new(),
            ignore: BTreeSet::new(),
            baseline_expiry: SignedDuration::from_hours(24 * 7),
        }
    }
}

/// What the container checks need: a source and a policy.
#[derive(Clone)]
pub struct ContainerDeps {
    /// Where containers are read from.
    pub source: Arc<dyn ContainerSource>,
    /// How particular containers are treated.
    pub policy: ContainerPolicy,
}

/// The containers from `deps` that the policy does not ignore.
pub(crate) async fn visible(deps: &ContainerDeps) -> Result<Vec<ContainerInfo>, CheckError> {
    let mut containers = deps.source.list().await?;
    containers.retain(|c| !deps.policy.ignore.contains(&c.name));
    containers.truncate(MAX_CONTAINERS);
    Ok(containers)
}
