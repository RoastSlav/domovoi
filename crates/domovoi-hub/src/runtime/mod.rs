//! Assembly and lifecycle of a complete hub.

pub(crate) mod adapters;
pub mod caps;
mod contain;
mod conversations_build;
mod delivery;
mod dispatcher;
mod error;
pub(crate) mod every;
pub mod first_run;
mod helper;
mod knowledge;
mod local;
mod lock;
mod orchestration;
mod packages;
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
use domovoi_core::ports::{
    AgentLink, ChatGateway, Clock, InteractionSink, LogSource, ProcessExecutor,
};
use domovoi_engine::{
    DomainEvent, Evidence, HostCatalog, IncidentManager, ManagerDeps, ManagerHandle,
    PatternRedactor,
};
use domovoi_store::Store;
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::agent_server::{AgentServer, AgentServerDeps, RealNetProbes, ServerSettings};
use caps::{action_caps, evidence_caps, observer_of};

use crate::api::{ApiDeps, ApiState, MetricsRender};
use crate::auth::{AuthState, WebIdentity};
use crate::digest::DigestDeps;
use crate::ingest::{IngestDeps, IngestState};
use crate::runtime::updates::UpdateChecker;

pub(crate) use dispatcher::Dispatcher;
pub use error::RuntimeError;
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
    /// The operation service, present only while `[orchestration] enabled` is set.
    pub(crate) operations_task: Option<JoinHandle<()>>,
    /// The install service's task.
    pub(crate) packages_task: JoinHandle<()>,
    pub(crate) profile_task: JoinHandle<()>,
    pub(crate) knowledge_task: JoinHandle<()>,
    /// The Operator conversation writer; it also runs the background answer turns.
    pub(crate) conversation_task: JoinHandle<()>,
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
        crate::packages::keys::PackageKeys::from_config(&config.packages)?;
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
        // Verdicts an older build stored on security events are withdrawn before anyone
        // reads them.
        if let Err(err) = domovoi_engine::triage::recap_security_verdicts(&*store, &manager).await {
            tracing::warn!(%err, "could not check stored security verdicts");
        }
        let server = AgentServer::new(
            ServerSettings::from_config(&config)?,
            AgentServerDeps {
                clock: clock.clone(),
                hosts: store.clone(),
                facts: store.clone(),
                sink: Arc::new(first_run::FirstRunSink::new(
                    Arc::new(manager.clone()),
                    store.clone(),
                    store.clone(),
                    clock.clone(),
                )),
                audit: store.clone(),
                probes: Arc::new(RealNetProbes::new(clock.clone())),
                hub_host,
            },
        )?;
        server.keep_package_reports(store.clone());
        if let Err(err) = server.restore_network_degraded(&*store).await {
            tracing::warn!(%err, "could not look for an open network incident");
        }
        let link: Arc<dyn AgentLink> = server.link();
        let helper_hub = helper::hub(&store, caps::helper_relay(link.clone()), &clock);
        let knowledge = knowledge::build(&store, &clock, helper_hub.clone());
        let registry = caps::registry(&link, &store, &clock).await?;
        // Built once, from the one link; each handle gets only its capabilities (ADR 0023).
        let evidence = Evidence::new(evidence_caps(link.clone()), clock.clone(), registry.clone());
        let catalog = HostCatalog::new(observer_of(link.clone()));
        let package_caps = caps::package_caps(link.clone());
        let unit_caps = caps::unit_caps(link.clone());
        let containment = contain::build(contain::ContainBuild {
            store: &store,
            caps: caps::contain_caps(link.clone()),
            trust: helper_hub.clone(),
            knowledge: knowledge.clone(),
            evidence: &evidence,
            units: unit_caps.clone(),
            clock: &clock,
        });
        let caps_bundle = action_caps(link);
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
            units: unit_caps.clone(),
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
                units: unit_caps.clone(),
                containment: containment.clone(),
            }) {
                Some((handle, task)) => (Some(handle), Some(task)),
                None => (None, None),
            };
        let packages = packages::build(packages::PackageBuild {
            config: &config,
            store: &store,
            caps: package_caps,
            units: unit_caps.clone(),
            registry: &registry,
            evidence: &evidence,
            triage: triage.0.as_ref(),
            clock: &clock,
            shutdown: &worker_shutdown,
        })
        .await?;
        server.set_reconciler(packages::reconciler(
            operations.as_ref(),
            containment.clone(),
            &packages.handle,
            helper::on_hello(&helper_hub),
        ));
        helper::on_status(&server, &helper_hub);
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
        let policy = policy::wire(policy::PolicyWire {
            store: &store,
            registry: &registry,
            evidence: &evidence,
            config: &config,
            clock: &clock,
            auth: &auth,
            shutdown: &worker_shutdown,
            units: &unit_caps,
        });
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
            knowledge: Some(knowledge.clone()),
        };
        crate::profile::backfill::backfill(&profile_deps).await;
        let updates = UpdateChecker::load(store.clone(), clock.clone()).await;
        let live = crate::api::operator::live::LiveBus::new();
        let (conversations, conversation_task) = conversations_build::build(
            &store,
            triage.0.clone(),
            &live,
            &redactor,
            &clock,
            &worker_shutdown,
        )
        .await?;
        let services = crate::services::Services::new(crate::services::ServicesDeps {
            manager: manager.clone(),
            triage: triage.0.clone(),
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
            knowledge: crate::services::KnowledgeCommands::new(
                knowledge.clone(),
                store.clone(),
                auth.clone(),
            ),
            conversations,
            settings: store.clone(),
            helper_trust: helper::service(&helper_hub, &store, &auth, &clock),
            containment: contain::services(&containment, &store, &auth, &clock),
            packages: crate::services::PackageCommands::new(crate::services::PackageDeps {
                handle: packages.handle.clone(),
                importer: packages.importer.clone(),
                installs: store.clone(),
                hosts: store.clone(),
                auth: auth.clone(),
                clock: clock.clone(),
            }),
        });
        let api = ApiState::new(
            ApiDeps {
                incidents: store.clone(),
                signals: store.clone(),
                hosts: store.clone(),
                notes: store.clone(),
                advisories: store.clone(),
                facts: store.clone(),
                usage: store.clone(),
                acks: store.clone(),
                uow: store.clone(),
                manager: manager.clone(),
                services: services.clone(),
                triage: triage.0,
                operation_repo: store.clone(),
                conversations: store.clone(),
                live,
                experience: store.clone(),
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
                audit_inspect: store.clone(),
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
            dispatcher.clone(),
            digest,
            updates,
        );
        let knowledge_task =
            knowledge::spawn_sweeper(knowledge, clock.clone(), intake_shutdown.clone());
        let profile_task = crate::profile::spawn(profile_deps, intake_shutdown.clone());
        helper::schedule(helper_hub, &store, intake_shutdown.clone());
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
            conversation_task,
            operations_task,
            packages_task: packages.task,
            profile_task,
            knowledge_task,
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
