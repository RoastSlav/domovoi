//! The agent runtime: wires the link, the scheduler and the request handler together.

use std::path::Path;
use std::sync::Arc;

use domovoi_checks::commands::Commands;
use domovoi_checks::containers::{ContainerDeps, ContainerPolicy, ContainerSource};
use domovoi_checks::framework::{HostDeps, Registry};
use domovoi_checks::system::FsStats;
use domovoi_config::AgentConfig;
use domovoi_core::ports::{Clock, HostFs, ProcessExecutor, Redactor};
use domovoi_runner::SkillCatalog;
use tokio_util::sync::CancellationToken;

use crate::checks::{CheckSet, check_settings};
use crate::error::AgentError;
use crate::identity::Identity;
use crate::link::backoff::random_jitter;
use crate::link::tls::PinnedVerifier;
use crate::link::ws::{Connector, TlsConnector};
use crate::link::{Link, LinkParts};
use crate::requests::{RequestCtx, serve};
use crate::scheduler::{CheckEnv, Scheduler};
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
    /// Filesystem usage for `disk.free` and the overview.
    pub fs_stats: Arc<dyn FsStats>,
    /// The container runtime; `None` turns the container checks and tools off.
    pub container_source: Option<Arc<dyn ContainerSource>>,
    /// Scrubs secrets from what is sent to the hub.
    pub redactor: Arc<dyn Redactor>,
}

/// A configured agent, ready to run.
pub struct AgentRuntime {
    identity: Identity,
    deps: AgentDeps,
    connector: Arc<dyn Connector>,
    checks: Arc<dyn CheckSet>,
    requests: Arc<RequestCtx>,
    store: Arc<CheckStore>,
    spool: Arc<SharedSpool>,
    relay: Option<crate::relay::Relay>,
    jitter: Box<dyn FnMut() -> f64 + Send + Sync>,
}

/// The container policy from `[containers]`.
pub(crate) fn container_policy(config: &AgentConfig) -> ContainerPolicy {
    ContainerPolicy {
        critical: config.containers.critical.iter().cloned().collect(),
        ignore: config.containers.ignore.iter().cloned().collect(),
        baseline_expiry: config.containers.baseline_expiry.as_signed(),
    }
}

fn load_catalog(dir: &Path) -> SkillCatalog {
    let (catalog, issues) = SkillCatalog::load(dir);
    for issue in issues {
        tracing::warn!(%issue, "skipped a skill");
    }
    catalog
}

impl AgentRuntime {
    /// Loads the identity, the skill catalogue, check settings and state for `config`.
    ///
    /// Fails with [`AgentError::NotEnrolled`] before enrollment.
    pub async fn new(config: AgentConfig, deps: AgentDeps) -> Result<Self, AgentError> {
        let state_dir = &config.agent.state_dir;
        let identity = Identity::load(state_dir)?
            .ok_or_else(|| AgentError::NotEnrolled(Identity::path(state_dir)))?;
        let settings = Arc::new(check_settings(&config).map_err(AgentError::Checks)?);
        let containers = deps.container_source.clone().map(|source| ContainerDeps {
            source,
            policy: container_policy(&config),
        });
        let host_deps = HostDeps {
            fs_stats: deps.fs_stats.clone(),
            containers,
        };
        let registry =
            Registry::build(&settings, host_deps).map_err(|e| AgentError::Checks(e.to_string()))?;
        let checks: Arc<dyn CheckSet> = Arc::new(registry);
        let commands = Arc::new(Commands::resolve(&*deps.hostfs).await);
        let store = Arc::new(CheckStore::open(state_dir)?);
        let spool = Arc::new(SharedSpool::new(Spool::open(state_dir)?));
        let check_env = CheckEnv {
            host: identity.host_id,
            fs: deps.hostfs.clone(),
            exec: deps.executor.clone(),
            commands: commands.clone(),
            clock: deps.clock.clone(),
            settings,
        };
        let requests = Arc::new(RequestCtx {
            host_id: identity.host_id,
            catalog: Arc::new(load_catalog(&config.agent.skills_dir)),
            allow_risk: config.policy.allow_risk.clone(),
            exec: deps.executor.clone(),
            redactor: deps.redactor.clone(),
            clock: deps.clock.clone(),
            fs: deps.hostfs.clone(),
            fs_stats: deps.fs_stats.clone(),
            commands,
            containers: deps.container_source.clone(),
            checks: checks.clone(),
            check_env,
            store: store.clone(),
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
        Ok(Self {
            identity,
            deps,
            connector,
            checks,
            requests,
            store,
            spool,
            relay,
            jitter: random_jitter(),
        })
    }

    /// Uses `connector` instead of dialling the hub over TLS, as single-box mode does with
    /// an in-memory link.
    #[must_use]
    pub fn with_connector(mut self, connector: Arc<dyn Connector>) -> Self {
        self.connector = connector;
        self
    }

    /// Uses `jitter` for reconnect delays instead of random factors.
    #[must_use]
    pub fn with_jitter(mut self, jitter: Box<dyn FnMut() -> f64 + Send + Sync>) -> Self {
        self.jitter = jitter;
        self
    }

    /// Runs until `shutdown` is cancelled or the link stops for good.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), AgentError> {
        let stop = shutdown.child_token();
        let host = self.identity.host_id;
        let parts = LinkParts {
            connector: self.connector,
            identity: self.identity,
            clock: self.deps.clock.clone(),
            fs: self.deps.hostfs.clone(),
            skills: self.requests.catalog.summaries(),
            spool: self.spool.clone(),
            jitter: self.jitter,
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
        let scheduler = tokio::spawn(scheduler.run(stop.clone()));
        let served = tokio::spawn(serve(self.requests, requests, handle, stop.clone()));
        let result = link.run(stop.clone()).await;
        stop.cancel();
        if let Err(err) = scheduler.await {
            tracing::error!(%err, "the scheduler stopped abnormally");
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
