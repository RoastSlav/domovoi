//! The real host adapters, wired from `agent.toml` (ARCHITECTURE.md R6: the agent crate
//! wires agent-side adapters; the binary injects the executor, clock and redactor).

use std::sync::Arc;

use domovoi_checks::containers::{ContainerSource, DockerSource};
use domovoi_checks::hostfs::RealHostFs;
use domovoi_checks::system::RealFsStats;
use domovoi_config::AgentConfig;
use domovoi_core::ports::{Clock, HostFs, ProcessExecutor, Redactor};

use crate::error::AgentError;
use crate::runtime::AgentDeps;

/// Production [`AgentDeps`]: files and filesystem usage under `[agent] host_root`, check
/// tools from the agent's own filesystem (where they run), and Docker at
/// `[docker] endpoint` when `[docker] enabled`.
pub fn deps(
    config: &AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    redactor: Arc<dyn Redactor>,
) -> Result<AgentDeps, AgentError> {
    let root = &config.agent.host_root;
    let container_source: Option<Arc<dyn ContainerSource>> = if config.docker.enabled {
        let source = DockerSource::connect(&config.docker.endpoint)
            .map_err(|err| AgentError::Checks(format!("[docker] endpoint: {err}")))?;
        Some(Arc::new(source))
    } else {
        None
    };
    Ok(AgentDeps {
        clock,
        executor,
        hostfs: Arc::new(RealHostFs::new(root.clone())),
        toolfs: Arc::new(RealHostFs::new("/")),
        fs_stats: Arc::new(RealFsStats::new(root.clone())),
        container_source,
        redactor,
    })
}

/// The host's files under `[agent] host_root`, for enrollment.
#[must_use]
pub fn host_fs(config: &AgentConfig) -> Arc<dyn HostFs> {
    Arc::new(RealHostFs::new(config.agent.host_root.clone()))
}

/// A certificate fingerprint as printed by `domovoi hub token create`: 64 hex digits,
/// with or without colons between the bytes.
pub fn parse_fingerprint(text: &str) -> Result<[u8; 32], String> {
    let digits: String = text.chars().filter(|c| *c != ':').collect();
    let bytes = hex::decode(digits.trim()).map_err(|err| format!("not hex: {err}"))?;
    bytes
        .try_into()
        .map_err(|_| "a fingerprint is 32 bytes (64 hex digits)".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domovoi_config::{AgentConfig, MapEnv};
    use domovoi_testkit::{FakeClock, FakeProcessExecutor, ListRedactor};

    use super::{deps, parse_fingerprint};

    #[tokio::test]
    async fn check_tools_are_looked_up_where_they_run_not_under_host_root() {
        // A container agent: the host is mounted at host_root, but smartctl and
        // journalctl run in the agent's own filesystem, so they must be found there.
        let host = tempfile::tempdir().unwrap();
        let tools = tempfile::tempdir().unwrap();
        let tool = tools.path().join("smartctl");
        std::fs::write(&tool, "").unwrap();
        let text = format!(
            "[agent]\nhost_name = \"nas\"\nhub_url = \"wss://hub:7421\"\nhost_root = '{}'\n",
            host.path().display()
        );
        let (config, _) = domovoi_config::parse::<AgentConfig>(&text, &MapEnv::new()).unwrap();
        let deps = deps(
            &config,
            Arc::new(FakeProcessExecutor::new()),
            Arc::new(FakeClock::new(jiff::Timestamp::UNIX_EPOCH)),
            Arc::new(ListRedactor::new(Vec::<String>::new())),
        )
        .unwrap();
        let tool = tool.to_str().unwrap();
        assert!(
            deps.toolfs.exists(tool).await,
            "tools come from the agent's filesystem"
        );
        assert!(
            !deps.hostfs.exists(tool).await,
            "host files come from host_root"
        );
    }

    #[test]
    fn fingerprints_parse_with_or_without_colons() {
        let plain = "ab".repeat(32);
        let colons = vec!["AB"; 32].join(":");
        assert_eq!(parse_fingerprint(&plain).unwrap(), [0xab; 32]);
        assert_eq!(parse_fingerprint(&colons).unwrap(), [0xab; 32]);
        assert!(parse_fingerprint("abcd").is_err());
        assert!(parse_fingerprint("zz").is_err());
    }
}
