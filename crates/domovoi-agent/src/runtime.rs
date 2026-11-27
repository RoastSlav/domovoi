//! The agent runtime: wires the link, the scheduler and the request handler together.

use std::path::Path;
use std::sync::Arc;

use domovoi_checks::commands::{Commands, ToolHost};
use domovoi_checks::containers::{ContainerDeps, ContainerPolicy, ContainerSource};
use domovoi_checks::framework::{HostDeps, Registry};
use domovoi_checks::system::FsStats;
use domovoi_config::AgentConfig;
use domovoi_core::ports::{Clock, HostFs, LogSource, PrivilegedReads, ProcessExecutor, Redactor};
use domovoi_proto::messages::HelperStatus;
use domovoi_runner::SkillCatalog;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::checks::{CheckSet, check_settings};
use crate::error::AgentError;
use crate::identity::Identity;
use crate::journal::Journal;
use crate::link::backoff::random_jitter;
use crate::link::handshake::HelloExtensions;
use crate::link::tls::PinnedVerifier;
use crate::link::ws::{Connector, TlsConnector};
use crate::link::{Link, LinkParts};
use crate::overrides::EffectiveChecks;
use crate::packages::{PackageDeps, PackageDirs, PackageService, TransferReceiver};
use crate::requests::{RequestCtx, serve};
use crate::scheduler::{CheckEnv, Scheduler};
use crate::skill_runner::SkillRoute;
use crate::spool::{SharedSpool, Spool};
use crate::store::CheckStore;

/// The adapters the binary injects.
#[derive(Clone)]
pub struct AgentDeps {
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Runs skills and check commands; the runner's sandbox in production.
    pub executor: Arc<dyn ProcessExecutor>,
    /// The host's files, under `[agent] host_root`.
    pub hostfs: Arc<dyn HostFs>,
    /// Where check tools such as `smartctl` are looked up: the filesystem they run in,
    /// which is the agent's own even when `host_root` points at a mounted host. With a
    /// `host_root` other than `/`, journalctl and systemctl are not used at all (see
    /// [`ToolHost`]).
    pub toolfs: Arc<dyn HostFs>,
    /// Filesystem usage for `disk.free` and the overview.
    pub fs_stats: Arc<dyn FsStats>,
    /// The container runtime; `None` turns the container checks and tools off.
    pub container_source: Option<Arc<dyn ContainerSource>>,
    /// The container runtime switch and what was found; `None` when nothing manages it
    /// (tests and one-off runs).
    pub docker: Option<crate::docker::DockerControl>,
    /// Scrubs secrets from what is sent to the hub.
    pub redactor: Arc<dyn Redactor>,
    /// The log lines of the process this agent runs in, served for `get_logs`;
    /// `None` when nothing captures them (tests and one-off runs).
    pub logs: Option<Arc<dyn LogSource>>,
    /// Whether the privilege boundary is in effect (the agent is the `domovoi-agent` account
    /// with exactly the expected groups), decided once at start by the binary.
    pub boundary_in_effect: bool,
    /// SMART, kernel-log and listening-socket reads served by the helper; `Some` only when the
    /// boundary is in effect, `None` keeps the checks' and tools' own commands.
    pub privileged: Option<Arc<dyn PrivilegedReads>>,
    /// How skills are executed: through the skill runner, or in the agent's own process for
    /// an agent running as root. Built by [`crate::production::skill_route`].
    pub skills: SkillRoute,
}

impl AgentDeps {}

/// A configured agent, ready to run.
pub struct AgentRuntime {
    effective: EffectiveChecks,
    check_infos: Vec<domovoi_proto::messages::CheckInfo>,
    identity: Identity,
    deps: AgentDeps,
    connector: Arc<dyn Connector>,
    checks: Arc<dyn CheckSet>,
    requests: Arc<RequestCtx>,
    journal: Arc<Journal>,
    store: Arc<CheckStore>,
    spool: Arc<SharedSpool>,
    relay: Option<crate::relay::Relay>,
    jitter: Box<dyn FnMut() -> f64 + Send + Sync>,
    state_dir: std::path::PathBuf,
    /// `allow_install` and `compressed_bytes` from `[packages]`, when it exists.
    transfer_policy: Option<(bool, u64)>,
    /// Whether the agent runs in a container, detected once at start.
    in_container: bool,
    /// The build named in `hello`.
    build: Option<String>,
    /// What the start-up banner reports.
    banner: crate::banner::Banner,
}

/// The container policy from `[containers]`.
pub(crate) fn container_policy(config: &AgentConfig) -> ContainerPolicy {
    ContainerPolicy {
        critical: config.containers.critical.iter().cloned().collect(),
        ignore: config.containers.ignore.iter().cloned().collect(),
        baseline_expiry: config.containers.baseline_expiry.as_signed(),
    }
}

/// Opens and recovers the action journal, propagating every ownership and metadata error.
///
/// A missing journal, a partial initialization, corrupt metadata or an ownership conflict is
/// fatal to agent startup. The agent never serves without the journal it owns.
async fn open_journal(config: &AgentConfig, deps: &AgentDeps) -> Result<Arc<Journal>, AgentError> {
    let dir = &config.agent.state_dir;
    let snapshot = crate::boot::boot_snapshot(&*deps.hostfs).await?;
    let journal = Journal::open(
        dir,
        &snapshot.boot_id.to_string(),
        snapshot.monotonic_ns,
        config.agent.mutation_limits.clone(),
    )?;
    journal.recover_orphans(&*deps.clock).await;
    Ok(Arc::new(journal))
}

#[cfg(target_os = "linux")]
pub(crate) fn runs_as_root() -> bool {
    nix::unistd::geteuid().is_root()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn runs_as_root() -> bool {
    false
}

/// The tool host: a container when detection says so or `host_root` is not `/`.
pub(crate) fn tool_host(host_root: &Path) -> ToolHost {
    if crate::container::in_container(host_root) {
        ToolHost::Container
    } else {
        ToolHost::from_host_root(host_root)
    }
}

fn load_catalog(dir: &Path) -> SkillCatalog {
    let (catalog, issues) = SkillCatalog::load(dir);
    if !dir.exists() {
        // One line that says what to do, not a raw "No such file or directory" per issue.
        tracing::warn!(
            skills_dir = %dir.display(),
            "the skills directory does not exist, so this agent has no skills; create it, or set \
             `skills_dir` in agent.toml (a package install puts them in /etc/domovoi/skills)"
        );
        return catalog;
    }
    for issue in issues {
        tracing::warn!(%issue, "skipped a skill");
    }
    catalog
}

/// Builds the package service when `agent.toml` has a `[packages]` section. An invalid
/// trusted key is fatal; an unusable directory only fences the service.
fn open_packages(
    config: &AgentConfig,
    deps: &AgentDeps,
) -> Result<Option<Arc<PackageService>>, AgentError> {
    let Some(section) = &config.packages else {
        return Ok(None);
    };
    let dirs = PackageDirs::new(
        config.agent.packages_dir.clone(),
        config.agent.state_dir.clone(),
        config.agent.skills_dir.clone(),
    );
    let package_deps =
        PackageDeps::production(deps.clock.clone(), runs_as_root(), deps.boundary_in_effect);
    let service = PackageService::open(section, dirs, package_deps)?;
    Ok(Some(Arc::new(service)))
}

impl AgentRuntime {
    /// Loads the identity, the skill catalogue, check settings and state for `config`.
    ///
    /// Fails with [`AgentError::NotEnrolled`] before enrollment.
    pub async fn new(config: AgentConfig, deps: AgentDeps) -> Result<Self, AgentError> {
        let state_dir = &config.agent.state_dir;
        let identity = Identity::load(state_dir)?
            .ok_or_else(|| AgentError::NotEnrolled(Identity::path(state_dir)))?;
        let base = check_settings(&config).map_err(AgentError::Checks)?;
        // Every check is built, so the hub can turn on one that `agent.toml` turns off;
        // the scheduler skips the ones off in the settings in effect.
        let mut all_on = base.clone();
        for settings in all_on.values_mut() {
            settings.enabled = true;
        }
        let containers = deps.container_source.clone().map(|source| ContainerDeps {
            source,
            policy: container_policy(&config),
        });
        let host_deps = HostDeps {
            fs_stats: deps.fs_stats.clone(),
            containers,
        };
        let registry =
            Registry::build(&all_on, host_deps).map_err(|e| AgentError::Checks(e.to_string()))?;
        let check_infos = crate::overrides::report(&registry, &base);
        let mut effective = EffectiveChecks::new(base);
        if let Some(docker) = &deps.docker {
            effective = effective.with_docker(docker.clone());
        }
        let checks: Arc<dyn CheckSet> = Arc::new(registry);
        let commands =
            Arc::new(Commands::resolve(&*deps.toolfs, tool_host(&config.agent.host_root)).await);
        let store = Arc::new(CheckStore::open(state_dir)?);
        let spool = Arc::new(SharedSpool::new(Spool::open(state_dir)?));
        let journal = open_journal(&config, &deps).await?;
        let packages = open_packages(&config, &deps)?;
        let check_env = CheckEnv {
            host: identity.host_id,
            fs: deps.hostfs.clone(),
            exec: deps.executor.clone(),
            commands: commands.clone(),
            clock: deps.clock.clone(),
            settings: effective.view(),
            privileged: deps.privileged.clone(),
        };
        let requests = Arc::new(RequestCtx {
            host_id: identity.host_id,
            catalog: Arc::new(load_catalog(&config.agent.skills_dir)),
            allow_risk: config.policy.allow_risk.clone(),
            exec: deps.executor.clone(),
            skills: deps.skills.clone(),
            redactor: deps.redactor.clone(),
            clock: deps.clock.clone(),
            fs: deps.hostfs.clone(),
            fs_stats: deps.fs_stats.clone(),
            commands,
            containers: deps.container_source.clone(),
            checks: checks.clone(),
            check_env,
            store: store.clone(),
            logs: deps.logs.clone(),
            journal: Some(journal.clone()),
            packages,
            helper: crate::helper_client::HelperRelay::default_path(),
            privileged: deps.privileged.clone(),
            ceiling: config
                .unattended
                .as_ref()
                .map(|section| Arc::new(crate::unattended::Ceiling::from_section(section))),
        });
        let verifier = Arc::new(PinnedVerifier::new(identity.hub_fingerprint));
        let connector = Arc::new(TlsConnector::new(
            config.agent.hub_url.clone(),
            verifier,
            deps.clock.clone(),
        ));
        let relay = match &config.local_webhooks {
            Some(relay) => {
                Some(crate::relay::Relay::bind(relay, config.agent.host_name.clone()).await?)
            }
            None => None,
        };
        let banner = crate::banner::Banner {
            host_name: config.agent.host_name.to_string(),
            hub_url: config.agent.hub_url.to_string(),
            config_file: None,
            state_dir: state_dir.clone(),
        };
        Ok(Self {
            build: None,
            banner,
            effective,
            check_infos,
            identity,
            deps,
            connector,
            checks,
            requests,
            journal,
            store,
            spool,
            relay,
            jitter: random_jitter(),
            state_dir: state_dir.clone(),
            in_container: crate::container::in_container(&config.agent.host_root),
            transfer_policy: config
                .packages
                .as_ref()
                .map(|section| (section.allow_install, section.limits.compressed_bytes)),
        })
    }

    /// Names the build in `hello`, so the hub can tell two builds of one release apart.
    /// `unknown` and an empty name are no name.
    #[must_use]
    pub fn with_build(mut self, build: &str) -> Self {
        self.build = Some(build.to_owned()).filter(|b| !b.is_empty() && b != "unknown");
        self
    }

    /// Names the configuration file the agent was started from, for the start-up banner.
    #[must_use]
    pub fn with_config_file(mut self, path: &Path) -> Self {
        self.banner.config_file = Some(path.to_path_buf());
        self
    }

    /// Uses `connector` instead of dialling the hub over TLS, as single-box mode does with
    /// an in-memory link.
    #[must_use]
    pub fn with_connector(mut self, connector: Arc<dyn Connector>) -> Self {
        self.connector = connector;
        self
    }

    /// Runs until `shutdown` is cancelled or the link stops for good.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), AgentError> {
        let stop = shutdown.child_token();
        let host = self.identity.host_id;
        self.banner.log(
            self.build.as_deref().unwrap_or("unknown"),
            host,
            self.identity.agent_id,
            self.check_infos.iter().filter(|c| c.enabled).count(),
            self.check_infos.len(),
        );
        let entries = crate::requests::catalog_entries_with(
            &self.requests.catalog,
            self.requests.package_surface(),
            &self.requests.enabled_package_skill_names(),
        );
        let helper = self.requests.helper.client().clone();
        let helper_status = crate::helper_client::probe_status(&helper, &*self.deps.clock).await;
        let helper_identity = match helper_status {
            HelperStatus::Ok { .. } => {
                crate::helper_client::probe_identity(&helper, &*self.deps.clock).await
            }
            _ => None,
        };
        // The runner is probed once, bounded, before `hello` is built, so the hub sees the real
        // status. A root agent runs skills itself (legacy route) and has nothing to probe.
        let runner = if let SkillRoute::Runner(route) = &self.deps.skills {
            route.refresh().await;
            Some(route.clone())
        } else {
            None
        };
        let extensions = HelloExtensions {
            agent_build: self.build,
            journal_epoch: self.journal.epoch(),
            runs_as_root: runs_as_root(),
            in_container: self.in_container,
            helper_status,
            helper_identity,
            skill_runner_status: runner
                .as_ref()
                .map_or_else(Default::default, |route| route.state().status()),
            catalog_hash: crate::link::handshake::catalog_announcement(&entries),
            ceiling_hash: self.requests.ceiling.as_ref().map(|c| c.hash()),
            ceiling_summary: self
                .requests
                .ceiling
                .as_ref()
                .map(|c| c.summary().to_owned()),
            ..HelloExtensions::default()
        };
        let status = crate::link::handshake::AgentStatusSource::new(extensions.status());
        let helper_probe = tokio::spawn(crate::helper_client::keep_status_with(
            helper,
            runner,
            self.deps.clock.clone(),
            status.clone(),
            stop.clone(),
        ));
        let (transfers, receiver) = match (&self.requests.packages, self.transfer_policy) {
            (Some(service), Some((allow_install, compressed_bytes))) => match service.staging() {
                Some(staging) => {
                    let (tx, rx) = mpsc::channel(64);
                    let receiver = TransferReceiver::new(
                        staging,
                        allow_install,
                        compressed_bytes,
                        self.deps.clock.clone(),
                        service.events(),
                    );
                    (Some(tx), Some((receiver, rx)))
                }
                None => (None, None),
            },
            _ => (None, None),
        };
        let parts = LinkParts {
            connector: self.connector,
            identity: self.identity,
            clock: self.deps.clock.clone(),
            fs: self.deps.hostfs.clone(),
            skills: self.requests.announced_skills().await,
            checks: self.check_infos,
            docker: self.deps.docker.as_ref().map(|d| d.report().clone()),
            extensions,
            status,
            overrides: Some(self.effective),
            spool: self.spool.clone(),
            jitter: self.jitter,
            status_dir: Some(self.state_dir),
            transfers,
            package_skills: Some(self.requests.clone()),
            resync: self
                .requests
                .packages
                .as_ref()
                .map(|service| service.skills_changed()),
        };
        let (link, handle, requests) = Link::new(parts);
        let scheduler = Scheduler::new(
            self.checks,
            self.requests.check_env.clone(),
            self.store.clone(),
            self.spool.clone(),
            handle.clone(),
        );
        let relay = self.relay.map(|relay| {
            tokio::spawn(relay.serve(
                self.spool.clone(),
                host,
                self.deps.clock.clone(),
                stop.clone(),
            ))
        });
        let transfers = receiver
            .map(|(receiver, rx)| tokio::spawn(receiver.serve(rx, handle.clone(), stop.clone())));
        let scheduler = tokio::spawn(scheduler.run(stop.clone()));
        let served = tokio::spawn(serve(self.requests, requests, handle, stop.clone()));
        let result = link.run(stop.clone()).await;
        stop.cancel();
        if let Err(err) = helper_probe.await {
            tracing::error!(%err, "the helper probe stopped abnormally");
        }
        if let Err(err) = scheduler.await {
            tracing::error!(%err, "the scheduler stopped abnormally");
        }
        if let Some(transfers) = transfers
            && let Err(err) = transfers.await
        {
            tracing::error!(%err, "the transfer receiver stopped abnormally");
        }
        if let Err(err) = served.await {
            tracing::error!(%err, "the request handler stopped abnormally");
        }
        let mut relay_error = None;
        if let Some(relay) = relay {
            match relay.await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => relay_error = Some(err),
                Err(err) => tracing::error!(%err, "the webhook relay stopped abnormally"),
            }
        }
        self.store.save()?;
        if let Some(err) = relay_error {
            return Err(AgentError::Relay(err));
        }
        result.map_err(AgentError::from)
    }
}
