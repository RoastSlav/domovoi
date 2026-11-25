//! The real host adapters, wired from `agent.toml` (ARCHITECTURE.md R6: the agent crate
//! wires agent-side adapters; the binary injects the executor, clock and redactor).

use std::sync::Arc;

use domovoi_checks::containers::{ContainerSource, Detection, detect};
use domovoi_checks::hostfs::RealHostFs;
use domovoi_checks::system::RealFsStats;
use domovoi_config::{AgentConfig, DockerMode};
use domovoi_core::ports::{Clock, HostFs, LogSource, PrivilegedReads, ProcessExecutor, Redactor};

use crate::docker::{self, DockerControl};
use crate::error::AgentError;
use crate::helper_client::HelperReads;
use crate::runtime::AgentDeps;
use crate::skill_runner::{ExpectedDirs, RunnerClient, RunnerRoute, SkillRoute};

/// The container runtime as the agent will use it: the switch and the source behind it.
struct DockerSetup {
    control: DockerControl,
    source: Option<Arc<dyn ContainerSource>>,
}

/// Looks for an engine on this host's own sockets, or at `[docker] endpoint`, and connects
/// to it. Looked for on every start, never remembered: an engine installed since the last
/// run is picked up by restarting the agent.
fn direct_docker(config: &AgentConfig) -> Result<DockerSetup, AgentError> {
    let mode = config.docker.enabled;
    let detection = (mode != DockerMode::Off).then(|| {
        detect(
            config.docker.endpoint.as_deref(),
            std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        )
    });
    let control = DockerControl::new(mode, detection.as_ref());
    log_detection(mode, detection.as_ref());
    let source =
        match docker::endpoint_for(mode, config.docker.endpoint.as_deref(), detection.as_ref()) {
            Some(endpoint) => {
                let source = docker::connect(&endpoint)
                    .map_err(|err| AgentError::Checks(format!("[docker] endpoint: {err}")))?;
                Some(control.gate(source))
            }
            None => None,
        };
    Ok(DockerSetup { control, source })
}

/// Whether the owner pointed `[docker] endpoint` at a network socket (a proxy), which is not
/// root-equivalent access and so stays a direct connection under the boundary (ADR 0018).
fn owner_network_endpoint(config: &AgentConfig) -> bool {
    config
        .docker
        .endpoint
        .as_deref()
        .is_some_and(|e| e.starts_with("tcp://") || e.starts_with("http://"))
}

/// The container runtime asked of the helper, which holds the engine socket the agent cannot
/// open. Nothing here falls back to a direct socket.
async fn helper_docker(config: &AgentConfig, helper: &HelperReads) -> DockerSetup {
    let mode = config.docker.enabled;
    let detection = match mode {
        DockerMode::Off => None,
        _ => Some(helper.detect_docker().await),
    };
    let control = DockerControl::new(mode, detection.as_ref());
    log_detection(mode, detection.as_ref());
    let source = control
        .report()
        .available
        .then(|| control.gate(helper.container_source()));
    DockerSetup { control, source }
}

/// Production [`AgentDeps`] for an agent whose privilege boundary is not in effect (a root
/// agent, a container, single-box mode): files and filesystem usage under `[agent]
/// host_root`, check tools from the agent's own filesystem (where they run), and the
/// container runtime found at start-up, or at `[docker] endpoint` (ADR 0018).
pub fn deps(
    config: &AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    redactor: Arc<dyn Redactor>,
    logs: Option<Arc<dyn LogSource>>,
) -> Result<AgentDeps, AgentError> {
    let docker = direct_docker(config)?;
    Ok(assemble(
        config, executor, clock, redactor, logs, docker, None,
    ))
}

/// [`deps`], or with the boundary in effect [`deps_via_helper`] over the helper socket.
pub async fn deps_with_boundary(
    config: &AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    redactor: Arc<dyn Redactor>,
    logs: Option<Arc<dyn LogSource>>,
    boundary_in_effect: bool,
) -> Result<AgentDeps, AgentError> {
    if boundary_in_effect {
        let helper = HelperReads::default_path(clock.clone());
        deps_via_helper(config, executor, clock, redactor, logs, &helper).await
    } else {
        deps(config, executor, clock, redactor, logs)
    }
}

/// Production [`AgentDeps`] for an agent whose privilege boundary is in effect: containers,
/// SMART, the kernel-log fallback and the listening-port owners are read through `helper`,
/// never through a unix socket or a privileged command of the agent's own. An owner-set
/// `tcp://` or `http://` `[docker] endpoint` keeps its direct connection.
pub async fn deps_via_helper(
    config: &AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    redactor: Arc<dyn Redactor>,
    logs: Option<Arc<dyn LogSource>>,
    helper: &HelperReads,
) -> Result<AgentDeps, AgentError> {
    let docker = if owner_network_endpoint(config) {
        direct_docker(config)?
    } else {
        helper_docker(config, helper).await
    };
    Ok(assemble(
        config,
        executor,
        clock,
        redactor,
        logs,
        docker,
        Some(helper.privileged()),
    ))
}

fn assemble(
    config: &AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    redactor: Arc<dyn Redactor>,
    logs: Option<Arc<dyn LogSource>>,
    docker: DockerSetup,
    privileged: Option<Arc<dyn PrivilegedReads>>,
) -> AgentDeps {
    let root = &config.agent.host_root;
    let skills = skill_route(
        config,
        executor.clone(),
        clock.clone(),
        crate::runtime::runs_as_root(),
    );
    AgentDeps {
        clock,
        executor,
        hostfs: Arc::new(RealHostFs::new(root.clone())),
        toolfs: Arc::new(RealHostFs::new("/")),
        fs_stats: Arc::new(RealFsStats::new(root.clone())),
        container_source: docker.source,
        docker: Some(docker.control),
        redactor,
        logs,
        boundary_in_effect: privileged.is_some(),
        privileged,
        skills,
    }
}

/// How this agent executes skills. An agent running as root (`euid_is_root`: the privilege
/// boundary is not in effect, a container or a host not yet migrated) spawns them itself
/// through `executor`, the labelled legacy path. Any other agent sends them to the skill
/// runner and never spawns one, whatever state the runner is in.
#[must_use]
pub fn skill_route(
    config: &AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    euid_is_root: bool,
) -> SkillRoute {
    if euid_is_root {
        return SkillRoute::Legacy(executor);
    }
    let expected = ExpectedDirs {
        skills: config.agent.skills_dir.clone(),
        packages: config.agent.packages_dir.clone(),
    };
    SkillRoute::Runner(Arc::new(RunnerRoute::new(
        RunnerClient::default_path(clock.clone()),
        expected,
        clock,
    )))
}

fn log_detection(mode: DockerMode, detection: Option<&Detection>) {
    match detection {
        Some(Detection::Found { endpoint, engine }) => {
            tracing::info!(%engine, %endpoint, "container runtime found");
        }
        Some(Detection::NotFound { endpoint, problem }) if mode == DockerMode::Auto => {
            if problem.contains("may not use") {
                tracing::warn!(%endpoint, %problem, "container runtime found but not usable");
            } else {
                tracing::info!(%problem, "no container runtime; the container checks are off");
            }
        }
        Some(Detection::NotFound { endpoint, problem }) => {
            tracing::warn!(%endpoint, %problem, "[docker] enabled = true, but nothing answered");
        }
        None => tracing::info!("[docker] enabled = false; the container checks are off"),
    }
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
            None,
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
