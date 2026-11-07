//! The container runtime of one host: what its agent found and the hub's choice (ADR 0018).

use domovoi_config::ConfigIssue;
use domovoi_core::ports::SettingsRepo;
use domovoi_core::{Host, User};
use domovoi_proto::messages::DockerReport;
use serde_json::{Value, json};

use super::host_docker_key;
use super::service::{SettingsError, SettingsService};

/// A host's container runtime: what the agent reported and what the hub chose.
#[derive(Debug, Clone, PartialEq)]
pub struct HostDocker {
    /// Whether the agent is connected now.
    pub connected: bool,
    /// What the agent reported at its last handshake, while it is connected. `None` from an
    /// agent too old to report it.
    pub report: Option<DockerReport>,
    /// The hub's saved choice; `None` leaves it to the agent.
    pub choice: Option<bool>,
}

impl HostDocker {
    /// Whether the runtime is in use on the host now, as far as the hub can tell.
    #[must_use]
    pub fn enabled(&self) -> bool {
        let Some(report) = &self.report else {
            return false;
        };
        if report.locked {
            report.enabled
        } else {
            report.available && self.choice.unwrap_or(true)
        }
    }
}

impl SettingsService {
    /// A host's container runtime state.
    pub async fn host_docker(&self, host: &Host) -> HostDocker {
        let deps = self.deps();
        let reported = deps.agents.reported_docker(host.id);
        HostDocker {
            connected: reported.is_some(),
            report: reported.flatten(),
            choice: crate::agent_server::stored_docker(&*deps.store, host.name.as_str()).await,
        }
    }

    /// Saves the hub's choice for a host and sends it to its agent: `Some` switches the
    /// runtime on or off, `None` goes back to what the agent decides. Returns whether the
    /// agent got it now; otherwise it gets it at its next handshake. No restart.
    ///
    /// Refused when the agent's own config fixes the setting, or when no engine was found
    /// on the host to switch on.
    pub async fn set_host_docker(
        &self,
        user: &User,
        host: &Host,
        choice: Option<bool>,
    ) -> Result<bool, SettingsError> {
        let current = self.host_docker(host).await;
        if let Some(why) = refusal(current.report.as_ref(), choice) {
            return Err(SettingsError::Invalid(vec![ConfigIssue::error(
                "docker", why,
            )]));
        }
        let deps = self.deps();
        let write = choice.map(Value::Bool);
        deps.store
            .set_many(&[(host_docker_key(host.name.as_str()), write)])
            .await?;
        self.audit(
            user,
            "settings.host_docker",
            &format!("host:{}", host.name),
            json!({ "choice": choice }),
        )
        .await;
        Ok(deps.agents.push_settings(host.id).await)
    }
}

/// Why `choice` cannot be applied to an agent that reported `report`, if it cannot. An agent
/// that has not reported (offline, or too old) is not refused: the choice waits for it.
fn refusal(report: Option<&DockerReport>, choice: Option<bool>) -> Option<String> {
    let report = report?;
    if report.locked {
        return Some(
            "agent.toml fixes this setting with `[docker] enabled`; change it there".to_owned(),
        );
    }
    if choice == Some(true) && !report.available {
        return Some(
            report
                .problem
                .clone()
                .unwrap_or_else(|| "no container engine was found on the host".to_owned()),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use domovoi_proto::messages::DockerReport;

    use super::{HostDocker, refusal};

    fn report(available: bool, locked: bool, enabled: bool) -> DockerReport {
        DockerReport {
            engine: "Docker".to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            available,
            locked,
            enabled,
            problem: (!available).then(|| "no Docker or Podman socket was found".to_owned()),
        }
    }

    #[test]
    fn the_hub_cannot_override_a_setting_the_agent_file_fixes() {
        let locked = report(true, true, true);
        assert!(refusal(Some(&locked), Some(false)).is_some());
        assert!(refusal(Some(&locked), None).is_some());
    }

    #[test]
    fn switching_on_needs_an_engine_but_switching_off_does_not() {
        let missing = report(false, false, false);
        let why = refusal(Some(&missing), Some(true)).unwrap();
        assert!(why.contains("no Docker or Podman socket"), "{why}");
        assert!(refusal(Some(&missing), Some(false)).is_none());
        assert!(refusal(Some(&missing), None).is_none());
    }

    #[test]
    fn an_agent_that_has_not_reported_is_not_refused() {
        assert!(refusal(None, Some(true)).is_none());
    }

    #[test]
    fn enabled_follows_the_choice_only_while_the_agent_leaves_it_open() {
        let state = |report, choice| HostDocker {
            connected: true,
            report,
            choice,
        };
        assert!(state(Some(report(true, false, true)), None).enabled());
        assert!(!state(Some(report(true, false, true)), Some(false)).enabled());
        assert!(!state(Some(report(false, false, false)), Some(true)).enabled());
        assert!(state(Some(report(true, true, true)), Some(false)).enabled());
        assert!(!state(None, Some(true)).enabled());
    }
}
