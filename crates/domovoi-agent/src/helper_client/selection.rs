//! What the agent builds when the privilege boundary is in effect: every Docker, SMART,
//! kernel-log and listening-socket read goes through one helper client, so they share one
//! cache and one rate limit.

use std::path::PathBuf;
use std::sync::Arc;

use domovoi_checks::containers::{ContainerSource, Detection};
use domovoi_core::ports::{Clock, PrivilegedReads};

use super::container_source::docker_problem;
use super::{HelperClient, HelperContainerSource, HelperPrivilegedReads};

/// The helper-backed sources of an agent whose boundary is in effect.
#[derive(Clone)]
pub struct HelperReads {
    client: HelperClient,
    clock: Arc<dyn Clock>,
}

impl HelperReads {
    /// Reads through the helper socket at `socket`.
    #[must_use]
    pub fn new(socket: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        Self {
            client: HelperClient::new(socket),
            clock,
        }
    }

    /// Reads through the socket the helper unit listens on.
    #[must_use]
    pub fn default_path(clock: Arc<dyn Clock>) -> Self {
        Self {
            client: HelperClient::default_path(),
            clock,
        }
    }

    /// The container source over the helper's `docker.read` group.
    #[must_use]
    pub fn container_source(&self) -> Arc<dyn ContainerSource> {
        Arc::new(HelperContainerSource::new(
            self.client.clone(),
            self.clock.clone(),
        ))
    }

    /// The SMART, kernel-log and listening-socket reads.
    #[must_use]
    pub fn privileged(&self) -> Arc<dyn PrivilegedReads> {
        Arc::new(HelperPrivilegedReads::new(
            self.client.clone(),
            self.clock.clone(),
        ))
    }

    /// Asks the helper which container engine it reaches. The agent cannot open the sockets
    /// itself, so it asks the helper.
    pub async fn detect_docker(&self) -> Detection {
        match self.client.docker_probe(&*self.clock).await {
            Ok(Some(info)) => Detection::Found {
                endpoint: info.endpoint,
                engine: if info.engine.eq_ignore_ascii_case("podman") {
                    "Podman"
                } else {
                    "Docker"
                },
            },
            Ok(None) => Detection::NotFound {
                endpoint: String::new(),
                problem: "no Docker or Podman socket was found".to_owned(),
            },
            Err(err) => Detection::NotFound {
                endpoint: String::new(),
                problem: docker_problem(&err),
            },
        }
    }
}
