//! Assembly and lifecycle of a complete hub.

pub(crate) mod adapters;
pub mod caps;
mod delivery;
mod dispatcher;
pub(crate) mod every;
mod local;
mod lock;
mod orchestration;
pub mod policy;
mod schedulers;
mod serve;
pub(crate) mod settings;
mod start;
mod support;
#[cfg(test)]
mod tests;
mod triage_build;
pub mod updates;

use std::sync::Arc;

use domovoi_config::{Secrets, hub::HubConfig};
use domovoi_core::capability::{ContractIndex, known_contracts};
use domovoi_core::ports::{
    AgentLink, ChatGateway, Clock, InteractionSink, LogSource, ProcessExecutor,
};
use domovoi_engine::{
    ActionDeps, ActionService, DomainEvent, Evidence, HostCatalog, IncidentManager, ManagerDeps,
    ManagerHandle, PatternRedactor,
};
use domovoi_llm::LlmBuildError;
use domovoi_store::{Store, StoreOpenError};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::agent_server::{
    AgentServer, AgentServerDeps, AgentServerError, RealNetProbes, ServerSettings,
};
use crate::capability::{HubRegistry, RegistryDeps, RegistryError};
use caps::{LinkCapabilities, action_caps, evidence_caps, observer_of};

use crate::api::{ApiDeps, ApiState, MetricsRender};
use crate::auth::{AuthState, WebIdentity};
use crate::digest::DigestDeps;
use crate::ingest::{IngestDeps, IngestState};
use crate::runtime::updates::UpdateChecker;

pub(crate) use dispatcher::Dispatcher;
pub use local::{LocalAgents, LocalEnrollment};
pub use lock::{HubLock, LockError};
pub(crate) mod backup_job;
pub(crate) use schedulers::{RuntimeContext, Schedulers};
pub(crate) use support::public_url as support_public_url;
pub(crate) use support::{AdapterOverrides, FanOut};

/// How many of the newest chained audit rows the hub checks at start.
pub const STARTUP_AUDIT_VERIFY_ROWS: u64 = 10_000;

/// External capabilities supplied by the binary. The hub never creates a process executor.
pub struct HubDeps {
    /// Time used by every runtime service and scheduler.
    pub clock: Arc<dyn Clock>,
    /// Runs a CLI language model; HTTP providers do not need it.
    pub process_executor: Option<Arc<dyn ProcessExecutor>>,
    /// Renders the process-wide metrics recorder the binary installed, for `/metrics`.
    /// `None` serves an empty page.
    pub metrics: Option<MetricsRender>,
    /// Where saved settings go to be applied: the `hub run` supervisor. `None` stores
    /// them for the next start.
    pub apply: Option<tokio::sync::mpsc::Sender<crate::settings::ApplyRequest>>,
    /// Recent log lines of this process, kept in memory by the binary that runs the hub.
    /// `None` means nothing was captured, where the API answers 503.
    pub logs: Option<Arc<dyn LogSource>>,
}

/// Why the hub could not be built, served or stopped.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Opening or migrating SQLite failed.
    #[error(transparent)]
    StoreOpen(#[from] StoreOpenError),
    /// Persisted runtime state could not be read or written.
    #[error(transparent)]
    Store(#[from] domovoi_core::ports::StoreError),
    /// The capability registry could not be built.
    #[error(transparent)]
    Registry(#[from] crate::capability::RegistryError),
    /// Agent-server settings or its TLS identity failed.
    #[error(transparent)]
    Agent(#[from] AgentServerError),
    /// A language model adapter could not be built.
    #[error(transparent)]
    Llm(#[from] LlmBuildError),
    /// A chat adapter could not be built.
    #[error("could not build a chat adapter: {0}")]
    Chat(String),
    /// A URL derived from configuration was invalid.
    #[error("invalid hub URL: {0}")]
    Url(String),
    /// A configured listener could not be opened.
    #[error("could not bind {name}: {source}")]
    Bind {
        /// Which listener failed.
        name: &'static str,
        /// The operating-system error.
        source: std::io::Error,
    },
    /// A server task stopped unexpectedly.
    #[error("{0}")]
    Serve(String),
    /// Another hub process holds this data directory.
    #[error(transparent)]
    Locked(#[from] LockError),
    /// The settings could not be loaded.
    #[error(transparent)]
    Settings(#[from] crate::settings::LoadError),
    /// The runtime exceeded its shutdown budget.
    #[error("hub shutdown did not complete within 15 seconds")]
    ShutdownTimeout,
}

/// A fully assembled hub. Build starts engine consumers; [`run`](Self::run) opens intake.
pub struct HubRuntime {
    pub(crate) lock: HubLock,
    pub(crate) auth: Arc<AuthState>,
    pub(crate) config: Arc<HubConfig>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) store: Arc<Store>,
    pub(crate) bus: broadcast::Sender<DomainEvent>,
    pub(crate) manager: ManagerHandle,
    pub(crate) agent_server: AgentServer,
    pub(crate) app: axum::Router,
    pub(crate) dispatcher: Dispatcher,
    pub(crate) events: broadcast::Receiver<DomainEvent>,
    pub(crate) schedulers: Schedulers,
    pub(crate) gateways: Vec<(Arc<dyn ChatGateway>, Arc<dyn InteractionSink>)>,
    pub(crate) manager_task: JoinHandle<()>,
    pub(crate) triage_task: Option<JoinHandle<()>>,
    pub(crate) actions_task: JoinHandle<()>,
    /// The operation service, present only while `[orchestration] enabled` is set.
    pub(crate) operations_task: Option<JoinHandle<()>>,
    pub(crate) profile_task: JoinHandle<()>,
    pub(crate) policy_task: Option<JoinHandle<()>>,
    pub(crate) intake_shutdown: CancellationToken,
    pub(crate) worker_shutdown: CancellationToken,
    pub(crate) manager_shutdown: CancellationToken,
    pub(crate) delivery_shutdown: CancellationToken,
}

impl HubRuntime {
    /// Builds adapters and engine services from validated, secret-resolved configuration.
    pub async fn build(config: HubConfig, deps: HubDeps) -> Result<Self, RuntimeError> {
        Self::build_with(config, deps, AdapterOverrides::default()).await
    }

    pub(crate) async fn build_with(
        config: HubConfig,
        deps: HubDeps,
        overrides: AdapterOverrides,
    ) -> Result<Self, RuntimeError> {
        let lock = HubLock::acquire(&config.hub.data_dir)?;
        let store = Arc::new(adapters::open_store(&config, &*deps.clock).await?);
        adapters::verify_audit_on_start(&store, &*deps.clock).await;
        let opened = start::Opened {
            lock,
            store,
            settings: None,
        };
        Self::assemble(opened, config, deps, overrides).await
    }

    pub(crate) async fn assemble(
        opened: start::Opened,
        mut config: HubConfig,
        deps: HubDeps,
        overrides: AdapterOverrides,
    ) -> Result<Self, RuntimeError> {
        let start::Opened {
            lock,
            store,
            settings,
        } = opened;
        let mut secrets = Vec::new();
        config.secrets_mut(&mut |_, secret| secrets.push(secret.value().clone()));
        let redactor = Arc::new(PatternRedactor::new(secrets));
        let clock = deps.clock;
        let test_executor = deps.process_executor.clone();
        adapters::ensure_hub_host(&store, &config, clock.now()).await?;
        let hub_host = adapters::hub_host(&store).await?;
        let capacity = overrides.bus_capacity.unwrap_or(4096).max(1);
        let (bus, _) = broadcast::channel(capacity);
        let manager_shutdown = CancellationToken::new();
        let worker_shutdown = CancellationToken::new();
        let delivery_shutdown = CancellationToken::new();
        let intake_shutdown = CancellationToken::new();
        let (manager, manager_task) = IncidentManager::spawn(ManagerDeps {
            incidents: store.clone(),
            uow: store.clone(),
            versions: store.clone(),
            acks: store.clone(),
            clock: clock.clone(),
            redactor: redactor.clone(),
            policy: settings::engine(&config),
            bus: bus.clone(),
            shutdown: manager_shutdown.clone(),
        });
        let server = AgentServer::new(
            ServerSettings::from_config(&config)?,
            AgentServerDeps {
                clock: clock.clone(),
                hosts: store.clone(),
                facts: store.clone(),
                sink: Arc::new(manager.clone()),
                audit: store.clone(),
                probes: Arc::new(RealNetProbes::new(clock.clone())),
                hub_host,
            },
        )?;
        let link: Arc<dyn AgentLink> = server.link();
        // The capability registry is the one place the compiled contracts, the agent's
        // catalogue, hub enablement, the skill floors and the ceiling meet (RW-10). The
        // engine gets it beside the narrow capabilities, never the link itself (ADR 0023).
        let registry = Arc::new(
            HubRegistry::new(RegistryDeps {
                contracts: Arc::new(
                    ContractIndex::new(known_contracts()).map_err(RegistryError::Index)?,
                ),
                agent: Arc::new(LinkCapabilities::new(link.clone())),
                hosts: store.clone(),
                store: store.clone(),
                clock: clock.clone(),
            })
            .await?,
        );
        // The one read boundary and the narrow catalogue view are built once here, from
        // the same link the engine dispatches through; each handle receives only
        // the capabilities it needs, never the link itself (ADR 0023).
        let evidence = Evidence::new(evidence_caps(link.clone()), clock.clone(), registry.clone());
        let catalog = HostCatalog::new(observer_of(link.clone()));
        let caps_bundle = action_caps(link);
        let (actions_service, actions_task) = ActionService::spawn(
            ActionDeps {
                actions: store.clone(),
                incidents: store.clone(),
                audit: store.clone(),
                uow: store.clone(),
                effects: store.clone(),
                advisories: store.clone(),
                probe: caps_bundle.probe.clone(),
                observer: caps_bundle.observer.clone(),
                journal: caps_bundle.journal.clone(),
                capabilities: registry.clone(),
                handoff: caps_bundle.handoff.clone(),
                rerun: caps_bundle.rerun.clone(),
                hosts: store.clone(),
                redactor: redactor.clone(),
                clock: clock.clone(),
                manager: manager.clone(),
                bus: bus.clone(),
                shutdown: worker_shutdown.clone(),
            },
            settings::actions(),
        );
        let triage = triage_build::build(triage_build::TriageBuild {
            config: &config,
            store: &store,
            evidence: evidence.clone(),
            catalog: catalog.clone(),
            capabilities: registry.clone(),
            manager: &manager,
            bus: &bus,
            clock: &clock,
            redactor: &redactor,
            executor: deps.process_executor,
            shutdown: &worker_shutdown,
            overrides: &overrides,
        })
        .await?;
        let (operations, operations_task) =
            match orchestration::build(orchestration::OperationBuild {
                config: &config,
                store: &store,
                evidence: &evidence,
                registry: &registry,
                caps: caps_bundle,
                triage: triage.0.as_ref(),
                redactor: &redactor,
                clock: &clock,
                shutdown: &worker_shutdown,
                bus: &bus,
            }) {
                Some((handle, task)) => (Some(handle), Some(task)),
                None => (None, None),
            };
        match &operations {
            Some(handle) => {
                server.set_reconciler(Arc::new(orchestration::CompositeReconciler::new(vec![
                    Arc::new(actions_service.clone()),
                    Arc::new(handle.clone()),
                ])));
            }
            None => server.set_reconciler(Arc::new(actions_service.clone())),
        }
        let public_url = support::public_url(&config)?;
        let (gateways, notifiers) = adapters::build_chat(&config, &clock, &public_url, &overrides)?;
        let identity = WebIdentity {
            public_url: public_url.clone(),
            trusted_proxies: config.web.trusted_proxies.clone(),
        };
        let auth = Arc::new(AuthState::new(
            store.clone(),
            store.clone(),
            clock.clone(),
            identity,
        ));
        let policy = policy::wire(
            &store,
            &registry,
            &evidence,
            &config,
            &clock,
            &auth,
            &worker_shutdown,
        );
        let settings = settings.map(|(key, sources)| {
            server.use_check_overrides(store.clone());
            crate::settings::SettingsService::new(crate::settings::SettingsDeps {
                store: store.clone(),
                key,
                sources,
                clock: clock.clone(),
                auth: auth.clone(),
                agents: server.clone(),
                apply: deps.apply.clone(),
                executor: test_executor,
            })
        });
        let profile_deps = crate::profile::ProfileDeps {
            evidence: evidence.clone(),
            catalog: catalog.clone(),
            capabilities: registry.clone(),
            agents: Arc::new(server.clone()),
            hosts: store.clone(),
            notes: store.clone(),
            settings: store.clone(),
            clock: clock.clone(),
            redactor: redactor.clone(),
        };
        let updates = UpdateChecker::load(store.clone(), clock.clone()).await;
        let services = crate::services::Services::new(crate::services::ServicesDeps {
            manager: manager.clone(),
            triage: triage.0.clone(),
            actions: store.clone(),
            actions_service: actions_service.clone(),
            operations: operations.clone(),
            operation_repo: store.clone(),
            incidents: store.clone(),
            hosts: store.clone(),
            facts: store.clone(),
            advisories: store.clone(),
            agents: Arc::new(server.clone()),
            profiles: Some(profile_deps.clone()),
            config: Arc::new(config.clone()),
            clock: clock.clone(),
            redactor: redactor.clone(),
            policy: policy.service,
        });
        let api = ApiState::new(
            ApiDeps {
                incidents: store.clone(),
                signals: store.clone(),
                actions: store.clone(),
                hosts: store.clone(),
                notes: store.clone(),
                advisories: store.clone(),
                facts: store.clone(),
                usage: store.clone(),
                acks: store.clone(),
                manager: manager.clone(),
                services: services.clone(),
                triage: triage.0,
                operation_repo: store.clone(),
                policy: store.clone(),
                agents: Arc::new(server.clone()),
                capabilities: registry.clone(),
                profiles: Some(profile_deps.clone()),
                clock: clock.clone(),
                journal: store.clone(),
                config: Arc::new(config.clone()),
                notifier: FanOut::over(&notifiers),
                metrics: deps.metrics,
                store_health: store.clone(),
                settings,
                audit: store.clone(),
                logs: deps.logs,
                redactor: redactor.clone(),
                update: updates.clone(),
            },
            auth.clone(),
        )
        .with_shutdown(intake_shutdown.clone());
        let ingest = IngestState::new(
            config.webhooks.clone(),
            IngestDeps {
                clock: clock.clone(),
                hosts: store.clone(),
                sink: Arc::new(manager.clone()),
            },
        );
        let app = crate::router::router(api.clone(), Some(ingest));
        let delivery_gateways = gateways.clone();
        let gateways = adapters::gateway_sinks(
            &config,
            &services,
            adapters::ChatReads::new(&store, &clock, FanOut::over(&notifiers), &public_url),
            gateways,
        );
        let config = Arc::new(config);
        let digest = DigestDeps {
            config: config.clone(),
            clock: clock.clone(),
            hosts: store.clone(),
            facts: store.clone(),
            incidents: store.clone(),
            advisories: store.clone(),
        };
        let events = bus.subscribe();
        let dispatcher = Dispatcher::new(
            config.clone(),
            clock.clone(),
            store.clone(),
            delivery_gateways,
            notifiers.clone(),
            settings::notify(&config),
        );
        let schedulers = Schedulers::new(
            config.clone(),
            RuntimeContext {
                clock: clock.clone(),
                store: store.clone(),
                redactor: redactor.clone(),
            },
            actions_service,
            dispatcher.clone(),
            digest,
            updates,
        );
        let profile_task = crate::profile::spawn(profile_deps, intake_shutdown.clone());
        Ok(Self {
            lock,
            auth,
            config,
            clock,
            store,
            bus,
            manager,
            agent_server: server,
            app,
            dispatcher,
            events,
            schedulers,
            gateways,
            manager_task,
            triage_task: triage.1,
            actions_task,
            operations_task,
            profile_task,
            policy_task: policy.task,
            intake_shutdown,
            worker_shutdown,
            manager_shutdown,
            delivery_shutdown,
        })
    }

    /// Registers and attaches agents running in this process (single-box mode).
    #[must_use]
    pub fn local_agents(&self) -> LocalAgents {
        LocalAgents::new(
            self.agent_server.clone(),
            self.store.clone(),
            self.clock.clone(),
        )
    }

    /// The first-run setup link while no account exists, issued once; `None` afterwards.
    pub async fn setup_link(&self) -> Result<Option<String>, RuntimeError> {
        self.auth
            .open_setup()
            .await
            .map_err(|err| RuntimeError::Serve(format!("could not open setup: {err}")))
    }

    /// Serves until `shutdown` is cancelled, then drains and closes within 15 seconds.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), RuntimeError> {
        serve::run(self, shutdown).await
    }
}
