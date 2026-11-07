//! The container runtime integration (ADR 0018): finding an engine, and the switch the hub
//! flips from the web UI.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_checks::containers::{
    ContainerError, ContainerEvent, ContainerInfo, ContainerSource, Detection, DockerSource,
};
use domovoi_config::DockerMode;
use domovoi_proto::messages::DockerReport;
use jiff::Timestamp;
use tokio::sync::watch;

/// The endpoint used when `[docker] enabled = true` names none and nothing answers.
const FALLBACK_ENDPOINT: &str = "unix:///var/run/docker.sock";

/// What the agent found and whether the integration is on. Cloning shares the switch.
#[derive(Clone)]
pub struct DockerControl {
    mode: DockerMode,
    available: bool,
    report: DockerReport,
    on: Arc<watch::Sender<bool>>,
}

impl std::fmt::Debug for DockerControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerControl")
            .field("report", &self.report)
            .field("on", &*self.on.borrow())
            .finish_non_exhaustive()
    }
}

impl DockerControl {
    /// Settles `mode` against what `detection` found. `detection` is `None` for
    /// `enabled = false`, which looks for nothing.
    #[must_use]
    pub fn new(mode: DockerMode, detection: Option<&Detection>) -> Self {
        let (endpoint, engine, problem, found) = match detection {
            Some(Detection::Found { endpoint, engine }) => {
                (endpoint.clone(), (*engine).to_owned(), None, true)
            }
            Some(Detection::NotFound { endpoint, problem }) => (
                endpoint.clone(),
                String::new(),
                Some(problem.clone()),
                false,
            ),
            None => (String::new(), String::new(), None, false),
        };
        // An explicit `true` is the owner's word that the engine is there, even if it did
        // not answer a moment ago.
        let available = found || mode == DockerMode::On;
        let report = DockerReport {
            engine,
            endpoint,
            available,
            locked: mode != DockerMode::Auto,
            enabled: initial(mode, available),
            problem: problem.filter(|_| !found),
        };
        Self {
            mode,
            available,
            on: Arc::new(watch::Sender::new(report.enabled)),
            report,
        }
    }

    /// What `hello` announces.
    #[must_use]
    pub fn report(&self) -> &DockerReport {
        &self.report
    }

    /// Whether the integration is in use now.
    #[must_use]
    pub fn is_on(&self) -> bool {
        *self.on.borrow()
    }

    /// Follows the integration being switched on or off.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<bool> {
        self.on.subscribe()
    }

    /// Applies the hub's choice. It has effect only when `agent.toml` leaves the setting to
    /// `auto` and an engine is available.
    pub fn choose(&self, choice: Option<bool>) {
        let on = match self.mode {
            DockerMode::Auto => self.available && choice.unwrap_or(true),
            DockerMode::On => self.available,
            DockerMode::Off => false,
        };
        let changed = self.on.send_if_modified(|current| {
            let changed = *current != on;
            *current = on;
            changed
        });
        if changed {
            tracing::info!(on, "the hub changed the container runtime setting");
        }
    }

    /// The container source to hand the agent: `source` behind this switch.
    #[must_use]
    pub fn gate(&self, source: Arc<dyn ContainerSource>) -> Arc<dyn ContainerSource> {
        Arc::new(Gated {
            inner: source,
            on: self.on.subscribe(),
        })
    }
}

fn initial(mode: DockerMode, available: bool) -> bool {
    match mode {
        DockerMode::Auto | DockerMode::On => available,
        DockerMode::Off => false,
    }
}

/// The source for `endpoint`, or why it cannot be built.
pub fn connect(endpoint: &str) -> Result<Arc<dyn ContainerSource>, ContainerError> {
    Ok(Arc::new(DockerSource::connect(endpoint)?))
}

/// The endpoint to connect to for `detection`, if there is one to try.
#[must_use]
pub fn endpoint_for(
    mode: DockerMode,
    configured: Option<&str>,
    detection: Option<&Detection>,
) -> Option<String> {
    match (detection, mode) {
        (Some(Detection::Found { endpoint, .. }), _) => Some(endpoint.clone()),
        (_, DockerMode::On) => Some(configured.unwrap_or(FALLBACK_ENDPOINT).to_owned()),
        _ => None,
    }
}

/// A source that refuses to be read while the integration is off.
struct Gated {
    inner: Arc<dyn ContainerSource>,
    on: watch::Receiver<bool>,
}

impl Gated {
    fn open(&self) -> Result<(), ContainerError> {
        if *self.on.borrow() {
            Ok(())
        } else {
            Err(ContainerError(
                "the container runtime is switched off for this host".to_owned(),
            ))
        }
    }
}

#[async_trait]
impl ContainerSource for Gated {
    async fn list(&self) -> Result<Vec<ContainerInfo>, ContainerError> {
        self.open()?;
        self.inner.list().await
    }

    async fn events_since(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<ContainerEvent>, ContainerError> {
        self.open()?;
        self.inner.events_since(since, until).await
    }

    async fn inspect(&self, name: &str) -> Result<serde_json::Value, ContainerError> {
        self.open()?;
        self.inner.inspect(name).await
    }

    async fn logs(&self, name: &str, lines: u16) -> Result<String, ContainerError> {
        self.open()?;
        self.inner.logs(name, lines).await
    }
}

#[cfg(test)]
mod tests {
    use domovoi_checks::containers::Detection;
    use domovoi_config::DockerMode;

    use super::DockerControl;

    fn found() -> Detection {
        Detection::Found {
            endpoint: "unix:///run/podman/podman.sock".to_owned(),
            engine: "Podman",
        }
    }

    fn missing() -> Detection {
        Detection::NotFound {
            endpoint: String::new(),
            problem: "no Docker or Podman socket was found".to_owned(),
        }
    }

    #[test]
    fn auto_uses_an_engine_that_answers_and_the_hub_may_switch_it_off() {
        let control = DockerControl::new(DockerMode::Auto, Some(&found()));
        assert!(control.is_on());
        assert!(!control.report().locked);
        assert_eq!(control.report().engine, "Podman");
        control.choose(Some(false));
        assert!(!control.is_on());
        control.choose(None);
        assert!(control.is_on(), "no choice goes back to the detected state");
    }

    #[test]
    fn auto_without_an_engine_stays_off_whatever_the_hub_says() {
        let control = DockerControl::new(DockerMode::Auto, Some(&missing()));
        assert!(!control.is_on());
        assert!(!control.report().available);
        assert!(control.report().problem.is_some());
        control.choose(Some(true));
        assert!(!control.is_on());
    }

    #[test]
    fn a_setting_in_the_file_cannot_be_changed_by_the_hub() {
        let on = DockerControl::new(DockerMode::On, Some(&missing()));
        assert!(on.is_on() && on.report().locked);
        on.choose(Some(false));
        assert!(on.is_on());

        let off = DockerControl::new(DockerMode::Off, None);
        assert!(!off.is_on() && off.report().locked);
        off.choose(Some(true));
        assert!(!off.is_on());
    }
}
