//! The helper's read-only view of the container engine.

use async_trait::async_trait;
use jiff::Timestamp;

use crate::helper::read::{ContainerRef, ContainerWire, DockerEngineInfo, EventWire};

/// Why an engine read failed. The helper maps each to a `ReadToken`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DockerReadError {
    /// No Docker or Podman socket exists.
    #[error("no container engine socket")]
    Absent,
    /// The socket exists and may not be opened.
    #[error("the container engine socket may not be opened")]
    Denied,
    /// The engine or a proxy answered 403 or 405 to a read.
    #[error("the container engine refused the read")]
    Refused,
    /// The engine answered 404.
    #[error("not found")]
    NotFound,
    /// A transport error or an unexpected status.
    #[error("container engine error: {0}")]
    Engine(String),
    /// The call took longer than its limit.
    #[error("the container engine timed out")]
    TimedOut,
}

/// Five reads of the container engine and nothing else: there is no method that changes a
/// container, so an implementation cannot be asked to.
#[async_trait]
pub trait DockerReadApi: Send + Sync {
    /// Finds the engine socket. Connects and sends nothing.
    async fn engine(&self) -> Result<DockerEngineInfo, DockerReadError>;
    /// Every container, running or not, sorted by name, at most 500.
    async fn list(&self) -> Result<Vec<ContainerWire>, DockerReadError>;
    /// Container events in `[since, until]`, at most 1 000.
    async fn events(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<EventWire>, DockerReadError>;
    /// The inspect document of `container`, with every `Config.Env` value already removed.
    async fn inspect(&self, container: &ContainerRef)
    -> Result<serde_json::Value, DockerReadError>;
    /// The last `lines` lines of the container's output, at most 256 KiB.
    async fn logs(&self, container: &ContainerRef, lines: u16) -> Result<String, DockerReadError>;
}
