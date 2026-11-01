//! Assembly and lifecycle of a complete hub.

pub(crate) mod adapters;
mod delivery;
mod dispatcher;
mod local;
mod lock;
mod schedulers;
mod serve;
mod settings;
mod support;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use domovoi_config::Secrets;
use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{ChatGateway, Clock, InteractionSink, Notifier, ProcessExecutor};
use domovoi_engine::triage::{TriageDeps, TriageHandle, TriageService};
use domovoi_engine::{
    ActionDeps, ActionService, DomainEvent, IncidentManager, ManagerDeps, ManagerHandle,
    PatternRedactor,
};
use domovoi_llm::LlmBuildError;
use domovoi_store::{Store, StoreOpenError};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent_server::{
    AgentServer, AgentServerDeps, AgentServerError, RealNetProbes, ServerSettings,
};
use crate::api::{ApiDeps, ApiState, MetricsRender};
use crate::auth::{AuthState, WebIdentity};
use crate::digest::DigestDeps;
use crate::ingest::{IngestDeps, IngestState};

pub(crate) use dispatcher::Dispatcher;
pub use local::{LocalAgents, LocalEnrollment};
pub use lock::{HubLock, LockError};
pub(crate) use schedulers::Schedulers;
pub(crate) use support::FanOut;
pub(crate) use support::public_url as support_public_url;

/// External capabilities supplied by the binary. The hub never creates a process executor.
pub struct HubDeps {
    /// Time used by every runtime service and scheduler.
    pub clock: Arc<dyn Clock>,
    /// Runs a CLI language model; HTTP providers do not need it.
    pub process_executor: Option<Arc<dyn ProcessExecutor>>,
    /// Renders the process-wide metrics recorder the binary installed, for `/metrics`.
    /// `None` serves an empty page.
    pub metrics: Option<MetricsRender>,
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
    /// The runtime exceeded its shutdown budget.
    #[error("hub shutdown did not complete within 15 seconds")]
    ShutdownTimeout,
}

#[derive(Default)]
pub(crate) struct AdapterOverrides {
    pub(crate) primary: Option<Arc<dyn domovoi_core::ports::LlmProvider>>,
    pub(crate) secondary: Option<Arc<dyn domovoi_core::ports::LlmProvider>>,
    pub(crate) gateways: Option<Vec<Arc<dyn ChatGateway>>>,
    pub(crate) notifiers: Option<Vec<Arc<dyn Notifier>>>,
    pub(crate) bus_capacity: Option<usize>,
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
        mut config: HubConfig,
        deps: HubDeps,
        overrides: AdapterOverrides,
    ) -> Result<Self, RuntimeError> {
        let lock = HubLock::acquire(&config.hub.data_dir)?;
        let mut secrets = Vec::new();
        config.secrets_mut(&mut |_, secret| secrets.push(secret.value().clone()));
        let redactor = Arc::new(PatternRedactor::new(secrets));
        let clock = deps.clock;
        let store = Arc::new(adapters::open_store(&config, &*clock).await?);
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
            signals: store.clone(),
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
                probes: Arc::new(RealNetProbes::new(clock.clone())),
                hub_host,
            },
        )?;
        let triage = build_triage(TriageBuild {
            config: &config,
            store: &store,
            server: &server,
            manager: &manager,
            bus: &bus,
            clock: &clock,
            redactor: &redactor,
            executor: deps.process_executor,
            shutdown: &worker_shutdown,
            overrides: &overrides,
        })
        .await?;
        let (actions_service, actions_task) = ActionService::spawn(
            ActionDeps {
                actions: store.clone(),
                incidents: store.clone(),
                audit: store.clone(),
                link: server.link(),
                redactor: redactor.clone(),
                clock: clock.clone(),
                manager: manager.clone(),
                bus: bus.clone(),
                shutdown: worker_shutdown.clone(),
            },
            settings::actions(),
        );
        let public_url = support::public_url(&config)?;
        let (gateways, notifiers) = adapters::build_chat(&config, &clock, &public_url, &overrides)?;
        let auth = Arc::new(AuthState::new(
            store.clone(),
            store.clone(),
            clock.clone(),
            WebIdentity {
                public_url: public_url.clone(),
                trusted_proxies: config.web.trusted_proxies.clone(),
            },
        ));
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
                manager: manager.clone(),
                actions_service: actions_service.clone(),
                triage: triage.0,
                agents: Arc::new(server.clone()),
                clock: clock.clone(),
                bus: bus.clone(),
                config: Arc::new(config.clone()),
                notifier: FanOut::over(&notifiers),
                metrics: deps.metrics,
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
        let gateways = adapters::gateway_sinks(&config, &api, &store, gateways);
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
            clock.clone(),
            store.clone(),
            actions_service,
            dispatcher.clone(),
            digest,
        );
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

struct TriageBuild<'a> {
    config: &'a HubConfig,
    store: &'a Arc<Store>,
    server: &'a AgentServer,
    manager: &'a ManagerHandle,
    bus: &'a broadcast::Sender<DomainEvent>,
    clock: &'a Arc<dyn Clock>,
    redactor: &'a Arc<PatternRedactor>,
    executor: Option<Arc<dyn ProcessExecutor>>,
    shutdown: &'a CancellationToken,
    overrides: &'a AdapterOverrides,
}

async fn build_triage(
    input: TriageBuild<'_>,
) -> Result<(Option<TriageHandle>, Option<JoinHandle<()>>), RuntimeError> {
    let TriageBuild {
        config,
        store,
        server,
        manager,
        bus,
        clock,
        redactor,
        executor,
        shutdown,
        overrides,
    } = input;
    if !config.triage.enabled {
        return Ok((None, None));
    }
    let primary = match &overrides.primary {
        Some(provider) => provider.clone(),
        None => domovoi_llm::build(
            &settings::provider(config.llm.primary.as_ref().ok_or_else(|| {
                RuntimeError::Chat("triage is enabled without a primary provider".to_owned())
            })?),
            executor.clone(),
        )?,
    };
    let secondary = match (&overrides.secondary, &config.llm.secondary) {
        (Some(provider), _) => Some(provider.clone()),
        (None, Some(config)) => Some(domovoi_llm::build(&settings::provider(config), executor)?),
        (None, None) => None,
    };
    let (handle, task) = TriageService::spawn(
        TriageDeps {
            primary,
            secondary,
            link: server.link(),
            incidents: store.clone(),
            signals: store.clone(),
            notes: store.clone(),
            actions: store.clone(),
            audit: store.clone(),
            cache: store.clone(),
            usage: store.clone(),
            advisories: store.clone(),
            redactor: redactor.clone(),
            clock: clock.clone(),
            manager: manager.clone(),
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        },
        settings::triage(config),
    );
    Ok((Some(handle), Some(task)))
}
