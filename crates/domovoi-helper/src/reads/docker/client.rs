//! The production [`DockerReadApi`]: five reads over a unix socket through `bollard`.
//!
//! This is the only file in the crate that uses `bollard`, and only `list_containers`,
//! `inspect_container`, `inspect_image`, `events`, `logs` and `connect_with_unix`.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use async_trait::async_trait;
use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::query_parameters::{
    EventsOptionsBuilder, ListContainersOptionsBuilder, LogsOptionsBuilder,
};
use domovoi_core::helper::read::{
    ContainerRef, ContainerWire, DOCKER_CALL_TIMEOUT_SECS, DOCKER_PROBE_TIMEOUT_SECS,
    DockerEngineInfo, EventWire, MAX_CONTAINERS, MAX_EVENTS, MAX_INSPECT_BYTES, MAX_LOG_TEXT_BYTES,
};
use domovoi_core::ports::{DockerReadApi, DockerReadError};
use futures::StreamExt;
use jiff::Timestamp;
use tokio::net::UnixStream;

use super::calls::{DockerCall, docker_time};
use super::map::{container_wire, event_wire, parse_time, truncate_utf8};
use super::sanitize::strip_env;

/// The system sockets tried, in order, when no endpoint is configured.
pub const DEFAULT_SOCKETS: [&str; 2] = ["/var/run/docker.sock", "/run/podman/podman.sock"];

/// Reads containers from a Docker or Podman socket without any way to change them.
#[derive(Debug, Clone)]
pub struct BollardDockerRead {
    candidates: Vec<String>,
}

/// An engine found by the probe.
struct Found {
    path: String,
    engine: &'static str,
}

fn is_denied(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(e) = current {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            return io.kind() == std::io::ErrorKind::PermissionDenied;
        }
        current = e.source();
    }
    false
}

fn map_error(err: &BollardError) -> DockerReadError {
    match err {
        BollardError::DockerResponseServerError { status_code, .. } => match status_code {
            403 | 405 => DockerReadError::Refused,
            404 => DockerReadError::NotFound,
            code => DockerReadError::Engine(format!("the engine answered {code}")),
        },
        BollardError::RequestTimeoutError => DockerReadError::TimedOut,
        BollardError::SocketNotFoundError(_) => DockerReadError::Absent,
        other if is_denied(other) => DockerReadError::Denied,
        BollardError::IOError { .. } | BollardError::HyperResponseError { .. } => {
            DockerReadError::Engine("transport error".to_owned())
        }
        BollardError::JsonDataError { .. } | BollardError::JsonSerdeError { .. } => {
            DockerReadError::Engine("unreadable engine answer".to_owned())
        }
        _ => DockerReadError::Engine("engine error".to_owned()),
    }
}

async fn timed<T>(
    limit: Duration,
    work: impl Future<Output = Result<T, DockerReadError>>,
) -> Result<T, DockerReadError> {
    tokio::time::timeout(limit, work)
        .await
        .unwrap_or(Err(DockerReadError::TimedOut))
}

fn call_limit() -> Duration {
    Duration::from_secs(DOCKER_CALL_TIMEOUT_SECS)
}

impl BollardDockerRead {
    /// Uses `configured` (`unix:///absolute/path`, already validated by config) when given,
    /// otherwise tries [`DEFAULT_SOCKETS`] in order. Nothing is opened here.
    #[must_use]
    pub fn new(configured: Option<&str>) -> Self {
        Self::with_candidates(configured, DEFAULT_SOCKETS.iter().map(|s| (*s).to_owned()))
    }

    /// Like [`Self::new`] with a different fallback list; tests point it at a tempdir.
    #[must_use]
    pub fn with_candidates(
        configured: Option<&str>,
        fallback: impl IntoIterator<Item = String>,
    ) -> Self {
        let candidates = match configured {
            Some(endpoint) => vec![
                endpoint
                    .strip_prefix("unix://")
                    .unwrap_or(endpoint)
                    .to_owned(),
            ],
            None => fallback.into_iter().collect(),
        };
        Self { candidates }
    }

    /// Connects to each candidate in turn and sends nothing. A socket that exists but may not
    /// be opened is reported only when no other candidate works.
    async fn probe(&self) -> Result<Found, DockerReadError> {
        let limit = Duration::from_secs(DOCKER_PROBE_TIMEOUT_SECS);
        let mut worst = DockerReadError::Absent;
        for path in &self.candidates {
            match tokio::time::timeout(limit, UnixStream::connect(path)).await {
                Ok(Ok(_)) => {
                    let engine = if path.contains("podman") {
                        "Podman"
                    } else {
                        "Docker"
                    };
                    return Ok(Found {
                        path: path.clone(),
                        engine,
                    });
                }
                Ok(Err(err)) => match err.kind() {
                    std::io::ErrorKind::NotFound => {}
                    std::io::ErrorKind::PermissionDenied => worst = DockerReadError::Denied,
                    _ if worst == DockerReadError::Absent => {
                        worst =
                            DockerReadError::Engine("the socket refused a connection".to_owned());
                    }
                    _ => {}
                },
                Err(_) if worst == DockerReadError::Absent => worst = DockerReadError::TimedOut,
                Err(_) => {}
            }
        }
        Err(worst)
    }

    async fn connect(&self) -> Result<Docker, DockerReadError> {
        let found = self.probe().await?;
        Docker::connect_with_unix(
            &format!("unix://{}", found.path),
            DOCKER_CALL_TIMEOUT_SECS,
            bollard::API_DEFAULT_VERSION,
        )
        .map_err(|e| map_error(&e))
    }

    async fn image_created(
        docker: &Docker,
        image_id: &str,
        cache: &mut HashMap<String, Option<Timestamp>>,
    ) -> Option<Timestamp> {
        if let Some(known) = cache.get(image_id) {
            return *known;
        }
        let created = match DockerCall::inspect_image(image_id) {
            Some(_) => docker
                .inspect_image(image_id)
                .await
                .ok()
                .and_then(|image| image.created.as_deref().and_then(parse_time)),
            None => None,
        };
        cache.insert(image_id.to_owned(), created);
        created
    }

    async fn list_inner(docker: &Docker) -> Result<Vec<ContainerWire>, DockerReadError> {
        let options = ListContainersOptionsBuilder::new().all(true).build();
        let summaries = docker
            .list_containers(Some(options))
            .await
            .map_err(|e| map_error(&e))?;
        let mut images = HashMap::new();
        let mut containers = Vec::new();
        for summary in summaries.into_iter().take(MAX_CONTAINERS + 1) {
            let Some(id) = summary.id else { continue };
            let inspect = match docker.inspect_container(&id, None).await {
                Ok(inspect) => inspect,
                // Removed between the listing and the inspect.
                Err(BollardError::DockerResponseServerError {
                    status_code: 404, ..
                }) => continue,
                Err(err) => return Err(map_error(&err)),
            };
            let created = match inspect.image.as_deref() {
                Some(image_id) => Self::image_created(docker, image_id, &mut images).await,
                None => None,
            };
            if let Some(wire) = container_wire(&inspect, created) {
                containers.push(wire);
            }
        }
        containers.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(containers)
    }

    async fn events_inner(
        docker: &Docker,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<EventWire>, DockerReadError> {
        let filters = HashMap::from([("type", vec!["container"])]);
        let options = EventsOptionsBuilder::new()
            .since(&docker_time(since))
            .until(&docker_time(until))
            .filters(&filters)
            .build();
        let mut stream = docker.events(Some(options)).take(MAX_EVENTS + 1);
        let mut events = Vec::new();
        while let Some(message) = stream.next().await {
            let message = message.map_err(|e| map_error(&e))?;
            if let Some(event) = event_wire(message, until) {
                events.push(event);
            }
        }
        Ok(events)
    }

    async fn logs_inner(
        docker: &Docker,
        container: &ContainerRef,
        lines: u16,
    ) -> Result<String, DockerReadError> {
        let options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .tail(&lines.to_string())
            .build();
        let mut stream = docker.logs(container.as_str(), Some(options));
        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| map_error(&e))?;
            bytes.extend_from_slice(&chunk.into_bytes());
            if bytes.len() > MAX_LOG_TEXT_BYTES {
                break;
            }
        }
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        truncate_utf8(&mut text, MAX_LOG_TEXT_BYTES);
        Ok(text)
    }
}

#[async_trait]
impl DockerReadApi for BollardDockerRead {
    async fn engine(&self) -> Result<DockerEngineInfo, DockerReadError> {
        let found = self.probe().await?;
        Ok(DockerEngineInfo {
            engine: found.engine.to_owned(),
            endpoint: found.path,
        })
    }

    /// Returns at most `MAX_CONTAINERS + 1` containers so the caller can tell a full list
    /// from a truncated one.
    async fn list(&self) -> Result<Vec<ContainerWire>, DockerReadError> {
        let docker = self.connect().await?;
        timed(call_limit(), Self::list_inner(&docker)).await
    }

    /// Returns at most `MAX_EVENTS + 1` events so the caller can tell truncation.
    async fn events(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<EventWire>, DockerReadError> {
        let docker = self.connect().await?;
        timed(call_limit(), Self::events_inner(&docker, since, until)).await
    }

    async fn inspect(
        &self,
        container: &ContainerRef,
    ) -> Result<serde_json::Value, DockerReadError> {
        let docker = self.connect().await?;
        timed(call_limit(), async {
            let inspect = docker
                .inspect_container(container.as_str(), None)
                .await
                .map_err(|e| map_error(&e))?;
            let mut value = serde_json::to_value(&inspect)
                .map_err(|_| DockerReadError::Engine("unreadable engine answer".to_owned()))?;
            strip_env(&mut value);
            let size = serde_json::to_vec(&value).map_or(usize::MAX, |v| v.len());
            if size > MAX_INSPECT_BYTES {
                return Err(DockerReadError::Engine(
                    "the inspect document is too large".to_owned(),
                ));
            }
            Ok(value)
        })
        .await
    }

    async fn logs(&self, container: &ContainerRef, lines: u16) -> Result<String, DockerReadError> {
        let docker = self.connect().await?;
        timed(call_limit(), Self::logs_inner(&docker, container, lines)).await
    }
}
