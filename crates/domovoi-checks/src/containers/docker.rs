//! [`ContainerSource`] over the Docker Engine API.

use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use bollard::Docker;
use bollard::models::{ContainerInspectResponse, ContainerStateStatusEnum, HealthStatusEnum};
use bollard::query_parameters::{
    EventsOptionsBuilder, ListContainersOptionsBuilder, LogsOptionsBuilder,
};
use futures::StreamExt;
use jiff::Timestamp;

use super::{
    ContainerError, ContainerEvent, ContainerInfo, ContainerSource, Health, MAX_CONTAINERS,
    MAX_LOG_BYTES, RunState, strip_env,
};

/// How long one Docker API request may take, in seconds.
pub const REQUEST_TIMEOUT_SECS: u64 = 10;
/// Most events one [`ContainerSource::events_since`] call returns.
pub const MAX_EVENTS: usize = 1000;

/// Reads containers from a Docker daemon or a socket proxy.
pub struct DockerSource {
    docker: Docker,
}

fn error(err: impl std::fmt::Display) -> ContainerError {
    ContainerError(err.to_string())
}

impl DockerSource {
    /// Connects to `endpoint`: `unix:///path/to/docker.sock` or `tcp://host:port`.
    ///
    /// Nothing is sent until the first request.
    pub fn connect(endpoint: &str) -> Result<Self, ContainerError> {
        let docker = if endpoint.starts_with("tcp://") || endpoint.starts_with("http://") {
            Docker::connect_with_http(endpoint, REQUEST_TIMEOUT_SECS, bollard::API_DEFAULT_VERSION)
                .map_err(error)?
        } else if endpoint.starts_with("unix://") {
            Self::connect_unix(endpoint)?
        } else {
            return Err(ContainerError(format!(
                "unsupported Docker endpoint {endpoint:?}; use unix:// or tcp://"
            )));
        };
        Ok(Self { docker })
    }

    #[cfg(unix)]
    fn connect_unix(endpoint: &str) -> Result<Docker, ContainerError> {
        Docker::connect_with_unix(endpoint, REQUEST_TIMEOUT_SECS, bollard::API_DEFAULT_VERSION)
            .map_err(error)
    }

    #[cfg(not(unix))]
    fn connect_unix(_endpoint: &str) -> Result<Docker, ContainerError> {
        Err(ContainerError(
            "unix:// Docker endpoints need a unix host".to_owned(),
        ))
    }

    async fn image_created(
        &self,
        image_id: &str,
        cache: &mut HashMap<String, Option<Timestamp>>,
    ) -> Option<Timestamp> {
        if let Some(known) = cache.get(image_id) {
            return *known;
        }
        let created = match self.docker.inspect_image(image_id).await {
            Ok(image) => image.created.as_deref().and_then(parse_time),
            Err(err) => {
                tracing::debug!(image_id, %err, "could not inspect image");
                None
            }
        };
        cache.insert(image_id.to_owned(), created);
        created
    }
}

/// A Docker timestamp; Docker's zero time (`0001-01-01T00:00:00Z`) is `None`.
#[must_use]
pub fn parse_time(text: &str) -> Option<Timestamp> {
    let at: Timestamp = text.parse().ok()?;
    (at > Timestamp::UNIX_EPOCH).then_some(at)
}

fn run_state(status: Option<ContainerStateStatusEnum>) -> RunState {
    match status {
        Some(ContainerStateStatusEnum::RUNNING) => RunState::Running,
        Some(ContainerStateStatusEnum::RESTARTING) => RunState::Restarting,
        Some(ContainerStateStatusEnum::PAUSED) => RunState::Paused,
        Some(ContainerStateStatusEnum::CREATED) => RunState::Created,
        Some(ContainerStateStatusEnum::DEAD | ContainerStateStatusEnum::REMOVING) => RunState::Dead,
        _ => RunState::Exited,
    }
}

fn health(status: Option<HealthStatusEnum>) -> Option<Health> {
    match status? {
        HealthStatusEnum::STARTING => Some(Health::Starting),
        HealthStatusEnum::HEALTHY => Some(Health::Healthy),
        HealthStatusEnum::UNHEALTHY => Some(Health::Unhealthy),
        _ => None,
    }
}

/// Maps one inspect response; `None` when it has no name.
#[must_use]
pub fn from_inspect(
    inspect: &ContainerInspectResponse,
    image_created: Option<Timestamp>,
) -> Option<ContainerInfo> {
    let name = inspect.name.as_deref()?.trim_start_matches('/').to_owned();
    if name.is_empty() {
        return None;
    }
    let state = inspect.state.as_ref();
    let config = inspect.config.as_ref();
    let labels: BTreeMap<String, String> = config
        .and_then(|c| c.labels.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();
    Some(ContainerInfo {
        name,
        image: config
            .and_then(|c| c.image.clone())
            .or_else(|| inspect.image.clone())
            .unwrap_or_default(),
        image_created,
        state: run_state(state.and_then(|s| s.status)),
        health: health(state.and_then(|s| s.health.as_ref()).and_then(|h| h.status)),
        restart_count: inspect
            .restart_count
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or(0),
        labels,
        auto_remove: inspect
            .host_config
            .as_ref()
            .and_then(|h| h.auto_remove)
            .unwrap_or(false),
        started_at: state
            .and_then(|s| s.started_at.as_deref())
            .and_then(parse_time),
    })
}

fn docker_time(at: Timestamp) -> String {
    format!("{}.{:09}", at.as_second(), at.subsec_nanosecond().max(0))
}

#[async_trait]
impl ContainerSource for DockerSource {
    async fn list(&self) -> Result<Vec<ContainerInfo>, ContainerError> {
        let options = ListContainersOptionsBuilder::new().all(true).build();
        let summaries = self
            .docker
            .list_containers(Some(options))
            .await
            .map_err(error)?;
        let mut images = HashMap::new();
        let mut containers = Vec::new();
        for summary in summaries.into_iter().take(MAX_CONTAINERS) {
            let Some(id) = summary.id else { continue };
            let inspect = match self.docker.inspect_container(&id, None).await {
                Ok(inspect) => inspect,
                // Removed between the listing and the inspect.
                Err(err) => {
                    tracing::debug!(container = id, %err, "could not inspect container");
                    continue;
                }
            };
            let image_created = match inspect.image.as_deref() {
                Some(image_id) => self.image_created(image_id, &mut images).await,
                None => None,
            };
            if let Some(info) = from_inspect(&inspect, image_created) {
                containers.push(info);
            }
        }
        containers.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(containers)
    }

    async fn events_since(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<ContainerEvent>, ContainerError> {
        let filters = HashMap::from([("type", vec!["container"])]);
        let options = EventsOptionsBuilder::new()
            .since(&docker_time(since))
            .until(&docker_time(until))
            .filters(&filters)
            .build();
        let mut stream = self.docker.events(Some(options)).take(MAX_EVENTS);
        let mut events = Vec::new();
        while let Some(message) = stream.next().await {
            let message = message.map_err(error)?;
            let attributes = message.actor.and_then(|a| a.attributes).unwrap_or_default();
            let Some(name) = attributes.get("name").cloned() else {
                continue;
            };
            let at = message
                .time_nano
                .and_then(|n| Timestamp::from_nanosecond(i128::from(n)).ok())
                .or_else(|| message.time.and_then(|s| Timestamp::from_second(s).ok()))
                .unwrap_or(until);
            events.push(ContainerEvent {
                at,
                name,
                action: message.action.unwrap_or_default(),
                image: attributes.get("image").cloned(),
            });
        }
        Ok(events)
    }

    async fn inspect(&self, name: &str) -> Result<serde_json::Value, ContainerError> {
        let inspect = self
            .docker
            .inspect_container(name, None)
            .await
            .map_err(error)?;
        let mut value = serde_json::to_value(&inspect).map_err(error)?;
        strip_env(&mut value);
        Ok(value)
    }

    async fn logs(&self, name: &str, lines: u16) -> Result<String, ContainerError> {
        let options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .tail(&lines.to_string())
            .build();
        let mut stream = self.docker.logs(name, Some(options));
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(error)?;
            text.push_str(&String::from_utf8_lossy(&chunk.into_bytes()));
            if text.len() >= MAX_LOG_BYTES {
                let mut end = MAX_LOG_BYTES;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
                break;
            }
        }
        Ok(text)
    }
}
