//! Assembly and lifecycle of a complete hub.

mod delivery;
mod dispatcher;
mod schedulers;
mod serve;
mod settings;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_config::Secrets;
use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{
    ChatGateway, Clock, HostRepo, InteractionSink, Notifier, ProcessExecutor, SettingsRepo,
};
use domovoi_core::{Host, HostId, HostKind, HostName};
use domovoi_engine::triage::{TriageDeps, TriageHandle, TriageService};
use domovoi_engine::{
    ActionDeps, ActionService, DomainEvent, IncidentManager, ManagerDeps, ManagerHandle,
    PatternRedactor,
};
use domovoi_llm::LlmBuildError;
use domovoi_store::{Store, StoreOpenError, StoreOptions};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent_server::{
    AgentServer, AgentServerDeps, AgentServerError, RealNetProbes, ServerSettings,
};
use crate::api::{ApiDeps, ApiState};
use crate::auth::interactions::{ChatDeps, InteractionAuthorizer};
use crate::auth::{AuthState, WebIdentity};
use crate::digest::DigestDeps;
use crate::ingest::{IngestDeps, IngestState};

pub(crate) use dispatcher::Dispatcher;
pub(crate) use schedulers::Schedulers;

/// External capabilities supplied by the binary. The hub never creates a process executor.
pub struct HubDeps {
    /// Time used by every runtime service and scheduler.
    pub clock: Arc<dyn Clock>,
    /// Runs a CLI language model; HTTP providers do not need it.
    pub process_executor: Option<Arc<dyn ProcessExecutor>>,
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
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut secrets = Vec::new();
        config.secrets_mut(&mut |_, secret| secrets.push(secret.value().clone()));
        let redactor = Arc::new(PatternRedactor::new(secrets));
        let clock = deps.clock;
        let database = config.hub.data_dir.join("hub.db");
        let store_options = StoreOptions {
            backup_dir: config
                .backup
                .dir
                .clone()
                .unwrap_or_else(|| config.hub.data_dir.join("backups")),
            keep_backups: config.backup.keep as usize,
            now: clock.now(),
        };
        let store = Arc::new(Store::open(&database, store_options).await?);
        ensure_hub_host(&store, &config, clock.now()).await?;
        let hub_host = hub_host(&store).await?;
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
        let public_url = public_url(&config)?;
        let (gateways, notifiers) = build_chat(&config, &clock, &public_url, &overrides)?;
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
                notifier: notifiers.first().cloned(),
                metrics: None,
            },
            auth,
        );
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
        let gateways = gateway_sinks(&config, &api, &store, gateways);
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
            notifiers,
        );
        Ok(Self {
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

    /// Serves until `shutdown` is cancelled, then drains and closes within 15 seconds.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), RuntimeError> {
        serve::run(self, shutdown).await
    }
}

async fn ensure_hub_host(
    store: &Store,
    config: &HubConfig,
    now: jiff::Timestamp,
) -> Result<(), RuntimeError> {
    if hub_host(store).await.is_ok() {
        return Ok(());
    }
    let name = HostName::parse(&config.hub.name)
        .or_else(|_| HostName::parse("domovoi"))
        .map_err(|err| RuntimeError::Url(err.to_string()))?;
    let id = if let Some(existing) = store.get_host_by_name(&name).await? {
        existing.id
    } else {
        let id = HostId::new();
        store
            .upsert_host(&Host {
                id,
                name,
                kind: HostKind::Local,
                labels: BTreeMap::new(),
                created_at: now,
            })
            .await?;
        id
    };
    SettingsRepo::set(store, "hub_host_id", &serde_json::json!(id.to_string())).await?;
    Ok(())
}

async fn hub_host(store: &Store) -> Result<HostId, RuntimeError> {
    let value = SettingsRepo::get(store, "hub_host_id")
        .await?
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| RuntimeError::Serve("the hub host id is not initialized".to_owned()))?;
    value
        .parse()
        .map_err(|err| RuntimeError::Serve(format!("invalid stored hub host id: {err}")))
}

fn public_url(config: &HubConfig) -> Result<url::Url, RuntimeError> {
    match &config.web.public_url {
        Some(url) => Ok(url.clone()),
        None => url::Url::parse(&format!("http://{}", config.web.bind))
            .map_err(|err| RuntimeError::Url(err.to_string())),
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

type ChatAdapters = (Vec<Arc<dyn ChatGateway>>, Vec<Arc<dyn Notifier>>);

fn build_chat(
    config: &HubConfig,
    clock: &Arc<dyn Clock>,
    public_url: &url::Url,
    overrides: &AdapterOverrides,
) -> Result<ChatAdapters, RuntimeError> {
    if overrides.gateways.is_some() || overrides.notifiers.is_some() {
        return Ok((
            overrides.gateways.clone().unwrap_or_default(),
            overrides.notifiers.clone().unwrap_or_default(),
        ));
    }
    let mut gateways: Vec<Arc<dyn ChatGateway>> = Vec::new();
    let mut notifiers: Vec<Arc<dyn Notifier>> = Vec::new();
    if let Some(discord) = &config.discord {
        if let Some(url) = &discord.webhook_url {
            notifiers.push(Arc::new(
                domovoi_chat::discord::DiscordWebhookNotifier::new(
                    "discord-webhook",
                    url.value().clone(),
                    clock.clone(),
                )
                .map_err(|err| RuntimeError::Chat(err.to_string()))?,
            ));
        }
        if discord.enabled {
            gateways.push(Arc::new(domovoi_chat::discord::DiscordGateway::new(
                discord.bot_token.value().clone(),
                domovoi_chat::discord::DiscordSettings {
                    channel_id: discord.channel_id,
                    web_base: public_url.clone(),
                },
                clock.clone(),
            )));
        }
    }
    Ok((gateways, notifiers))
}

fn gateway_sinks(
    config: &HubConfig,
    api: &ApiState,
    store: &Arc<Store>,
    gateways: Vec<Arc<dyn ChatGateway>>,
) -> Vec<(Arc<dyn ChatGateway>, Arc<dyn InteractionSink>)> {
    let allowed = config
        .discord
        .as_ref()
        .map(|discord| {
            discord
                .allowed_user_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    gateways
        .into_iter()
        .map(|gateway| {
            let sink: Arc<dyn InteractionSink> = Arc::new(InteractionAuthorizer::new(
                api,
                ChatDeps {
                    users: store.clone(),
                    notify: store.clone(),
                    audit: store.clone(),
                    gateway: gateway.clone(),
                    allowed: allowed.clone(),
                },
            ));
            (gateway, sink)
        })
        .collect()
}
