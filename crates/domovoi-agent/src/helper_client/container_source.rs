//! The container source that reads through the helper's `docker.read` group.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_checks::containers::{
    ContainerError, ContainerEvent, ContainerInfo, ContainerSource, Health, MAX_CONTAINERS,
    RunState,
};
use domovoi_core::helper::UnavailableReason;
use domovoi_core::helper::read::{
    ContainerRef, ContainerWire, MAX_EVENT_WINDOW_SECS, MAX_LOG_LINES, ReadToken, WireHealth,
    WireRunState,
};
use domovoi_core::ports::Clock;
use jiff::{SignedDuration, Timestamp};

use super::{HelperClient, ReadClientError};

/// A [`ContainerSource`] whose every call is one helper read.
#[derive(Clone)]
pub struct HelperContainerSource {
    client: HelperClient,
    clock: Arc<dyn Clock>,
}

impl HelperContainerSource {
    /// A source that reads through `client`, timing every exchange on `clock`.
    #[must_use]
    pub fn new(client: HelperClient, clock: Arc<dyn Clock>) -> Self {
        Self { client, clock }
    }
}

/// The sentence for a failed Docker read: what `hello.docker` shows as the problem and what a
/// container check reports.
#[must_use]
pub fn docker_problem(err: &ReadClientError) -> String {
    let unusable = |what: &str| format!("the helper is not usable ({what})");
    match err {
        ReadClientError::Missing => "the helper is not installed; Docker is read through it".into(),
        ReadClientError::Unreachable => unusable("unreachable"),
        ReadClientError::Protocol => unusable("protocol"),
        ReadClientError::StartRefused(token) | ReadClientError::Refused(token) => {
            unusable(token.as_str())
        }
        ReadClientError::Unavailable(UnavailableReason::GroupDisabled) => {
            "docker.read is switched off in helper.toml".into()
        }
        ReadClientError::Unavailable(UnavailableReason::PlatformBlocked) => {
            "this platform's isolation checks failed; run domovoi agent selftest".into()
        }
        ReadClientError::Unavailable(_) => unusable("unavailable"),
        ReadClientError::Failed(ReadToken::EngineAbsent, _) => {
            "no Docker or Podman socket was found".into()
        }
        ReadClientError::Failed(ReadToken::EngineDenied | ReadToken::EngineError, _) => {
            "the helper cannot use the container engine socket".into()
        }
        ReadClientError::Failed(ReadToken::EngineApiRefused, _) => {
            "the container engine refused the read".into()
        }
        ReadClientError::Failed(ReadToken::TargetNotFound, _) => "no such container".into(),
        ReadClientError::Failed(ReadToken::TimedOut, _) => "the container engine timed out".into(),
        ReadClientError::Failed(token, _) => format!("the helper read failed ({token})"),
        ReadClientError::RateLimited => err.to_string(),
    }
}

fn failed(err: ReadClientError) -> ContainerError {
    ContainerError(docker_problem(&err))
}

fn run_state(state: WireRunState) -> RunState {
    match state {
        WireRunState::Running => RunState::Running,
        WireRunState::Restarting => RunState::Restarting,
        WireRunState::Paused => RunState::Paused,
        WireRunState::Created => RunState::Created,
        WireRunState::Exited => RunState::Exited,
        WireRunState::Dead => RunState::Dead,
    }
}

fn health(health: WireHealth) -> Health {
    match health {
        WireHealth::Starting => Health::Starting,
        WireHealth::Healthy => Health::Healthy,
        WireHealth::Unhealthy => Health::Unhealthy,
    }
}

fn container_info(wire: ContainerWire) -> ContainerInfo {
    ContainerInfo {
        name: wire.name,
        image: wire.image,
        image_created: wire.image_created,
        state: run_state(wire.state),
        health: wire.health.map(health),
        restart_count: wire.restart_count,
        labels: wire.labels,
        auto_remove: wire.auto_remove,
        started_at: wire.started_at,
    }
}

fn container_ref(name: &str) -> Result<ContainerRef, ContainerError> {
    ContainerRef::parse(name).map_err(|_| ContainerError("invalid container name".to_owned()))
}

#[async_trait]
impl ContainerSource for HelperContainerSource {
    async fn list(&self) -> Result<Vec<ContainerInfo>, ContainerError> {
        let (containers, _) = self
            .client
            .docker_list(&*self.clock)
            .await
            .map_err(failed)?;
        Ok(containers
            .into_iter()
            .take(MAX_CONTAINERS)
            .map(container_info)
            .collect())
    }

    async fn events_since(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<ContainerEvent>, ContainerError> {
        let now = self.clock.now();
        let floor = now - SignedDuration::from_secs(i64::from(MAX_EVENT_WINDOW_SECS));
        let since = since.max(floor);
        if since.as_second() >= until.as_second() {
            return Ok(Vec::new());
        }
        let (events, _) = self
            .client
            .docker_events(&*self.clock, since, until)
            .await
            .map_err(failed)?;
        Ok(events
            .into_iter()
            .map(|e| ContainerEvent {
                at: e.at,
                name: e.name,
                action: e.action,
                image: e.image,
            })
            .collect())
    }

    async fn inspect(&self, name: &str) -> Result<serde_json::Value, ContainerError> {
        let container = container_ref(name)?;
        self.client
            .docker_inspect(&*self.clock, &container)
            .await
            .map_err(failed)
    }

    async fn logs(&self, name: &str, lines: u16) -> Result<String, ContainerError> {
        let container = container_ref(name)?;
        let lines = lines.clamp(1, MAX_LOG_LINES);
        let (text, _) = self
            .client
            .docker_logs(&*self.clock, &container, lines)
            .await
            .map_err(failed)?;
        Ok(text)
    }
}
