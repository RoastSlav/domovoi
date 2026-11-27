//! The agent's start-up banner: one INFO line saying which build and configuration runs.

use std::path::PathBuf;

use domovoi_core::{AgentId, HostId};

/// What the banner reports, besides what the runtime already knows.
#[derive(Debug, Clone)]
pub struct Banner {
    /// This host's name in Domovoi.
    pub host_name: String,
    /// The hub's agent server address.
    pub hub_url: String,
    /// The agent configuration file that was read, if known.
    pub config_file: Option<PathBuf>,
    /// The state directory.
    pub state_dir: PathBuf,
}

/// The name of the account the agent runs as, or `unknown`.
#[cfg(target_os = "linux")]
fn current_user() -> String {
    nix::unistd::User::from_uid(nix::unistd::geteuid())
        .ok()
        .flatten()
        .map_or_else(|| "unknown".to_owned(), |user| user.name)
}

#[cfg(not(target_os = "linux"))]
fn current_user() -> String {
    "unknown".to_owned()
}

impl Banner {
    /// Logs the banner. `commit` is the build's git commit; `enabled` and `total` count the
    /// checks `agent.toml` leaves on and all the checks this build has.
    pub fn log(
        &self,
        commit: &str,
        host_id: HostId,
        agent_id: AgentId,
        enabled: usize,
        total: usize,
    ) {
        tracing::info!(
            version = %env!("CARGO_PKG_VERSION"),
            commit = %commit,
            host = %self.host_name,
            host_id = %host_id,
            agent_id = %agent_id,
            hub_url = %self.hub_url,
            config_file = %self
                .config_file
                .as_ref()
                .map_or_else(|| "unknown".to_owned(), |p| p.display().to_string()),
            state_dir = %self.state_dir.display(),
            user = %current_user(),
            checks_enabled = enabled,
            checks_total = total,
            "domovoi agent starting",
        );
    }
}

#[cfg(test)]
mod tests {
    use domovoi_testkit::log_capture::LogCapture;

    use super::*;

    #[test]
    fn the_banner_names_build_host_hub_and_files() {
        let (logs, _guard) = LogCapture::start();
        let banner = Banner {
            host_name: "kuker".to_owned(),
            hub_url: "wss://hub.example:7421".to_owned(),
            config_file: Some(PathBuf::from("/etc/domovoi/agent.toml")),
            state_dir: PathBuf::from("/var/lib/domovoi-agent"),
        };
        banner.log("abc1234", HostId::new(), AgentId::new(), 7, 9);
        let lines = logs.lines_with("domovoi agent starting");
        assert_eq!(lines.len(), 1, "{}", logs.text());
        for field in [
            "INFO",
            "commit=abc1234",
            "host=kuker",
            "host_id=",
            "agent_id=",
            "hub_url=wss://hub.example:7421",
            "config_file=/etc/domovoi/agent.toml",
            "state_dir=/var/lib/domovoi-agent",
            "user=",
            "checks_enabled=7",
            "checks_total=9",
        ] {
            assert!(
                lines[0].contains(field),
                "banner lacks {field}: {}",
                lines[0]
            );
        }
    }
}
