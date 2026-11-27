//! The hub API over a temporary SQLite store, a running incident manager and action
//! service, fakes for agents and time, and the example `hub.toml` with test secrets.

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use domovoi_config::{HubConfig, MapEnv};
use domovoi_core::capability::{ContractIndex, known_contracts};
use domovoi_core::ports::{
    AuditEvent, AuditFilter, AuditLog, Clock, HostRepo, IncidentRepo, IncidentTimelineEntry,
    LinkError, LlmProvider, LogLine, LogSource, Notifier, Page, Presence, StoreHealth,
};
use domovoi_core::{HostId, HostName, Incident, SkillSummary};
use domovoi_engine::{
    DomainEvent, EnginePolicy, IncidentManager, ManagerDeps, ManagerHandle, PatternRedactor,
    TriageDeps, TriageService, TriageSettings,
};
use domovoi_hub::agent_server::{
    AgentServer, AgentServerDeps, AgentServerError, EnrollmentInvite, RealNetProbes, ServerSettings,
};
use domovoi_hub::api::{AgentAdmin, ApiDeps, ApiState};
use domovoi_hub::auth::{AuthState, WebIdentity};
use domovoi_hub::capability::{HubRegistry, RegistryDeps};
use domovoi_hub::profile::ProfileDeps;
use domovoi_hub::router::router;
use domovoi_hub::runtime::updates::UpdateChecker;
use domovoi_hub::services::{PolicyDeps, PolicyService, Services, ServicesDeps};
use domovoi_hub::settings::{ConfigSources, SecretKey, SettingsDeps, SettingsService};
use domovoi_proto::EnrollToken;
use domovoi_proto::messages::{CheckInfo, HostFacts};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{host, open_incident};
use domovoi_testkit::link::{evidence_caps, observer_of};
use domovoi_testkit::{
    CannedLogSource, FakeAgentLink, FakeClock, RecordingSignalSink, ScriptedLlm,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// The origin of the example config's `public_url`.
pub const ORIGIN: &str = "https://domovoi.example.net";

/// Secret values the example config reads; none may ever appear in a response.
pub const SECRETS: [(&str, &str); 7] = [
    ("ANTHROPIC_API_KEY", "sk-ant-test-0123456789abcdef"),
    ("DISCORD_BOT_TOKEN", "discord-bot-token-not-for-eyes"),
    (
        "DISCORD_WEBHOOK_URL",
        "https://discord.com/api/webhooks/1/hook-secret",
    ),
    ("DOMOVOI_ALERTS_TOKEN", "alerts-token-not-for-eyes"),
    (
        "DOMOVOI_HEARTBEAT_URL",
        "https://hc.example/ping/heartbeat-secret",
    ),
    ("DOMOVOI_METRICS_TOKEN", "metrics-token-not-for-eyes"),
    ("OLLAMA_API_KEY", "ollama-key-not-for-eyes"),
];

/// The admin's credentials.
pub const USERNAME: &str = "admin";
pub const PASSWORD: &str = "correct horse battery";

/// The client address of every request.
pub fn peer() -> SocketAddr {
    SocketAddr::from(([10, 0, 0, 7], 50_000))
}

pub fn t0() -> Timestamp {
    "2026-10-31T12:00:00Z".parse().unwrap()
}

/// Log lines the harness's log source answers with, oldest first. The last one carries
/// an `sk-` shaped credential so a test can see it removed from the response.
pub fn canned_logs() -> Vec<LogLine> {
    [
        LogLine {
            at: "2026-10-31T12:00:01Z".parse().unwrap(),
            level: "info".to_owned(),
            target: "domovoi_hub.runtime".to_owned(),
            message: "hub started hosts=1".to_owned(),
        },
        LogLine {
            at: "2026-10-31T12:00:02Z".parse().unwrap(),
            level: "warn".to_owned(),
            target: "domovoi_hub.checks".to_owned(),
            message: "disk.usage on /srv is 91% full".to_owned(),
        },
        LogLine {
            at: "2026-10-31T12:00:03Z".parse().unwrap(),
            level: "error".to_owned(),
            target: "domovoi_hub.chat".to_owned(),
            message: "gateway rejected key=sk-ant-test-0123456789abcdef".to_owned(),
        },
    ]
    .to_vec()
}

/// Agents for the API: presence and catalogues from a fake link; invites and revocations
/// recorded in the store as the real agent server would.
pub struct TestAgents {
    pub link: Arc<FakeAgentLink>,
    pub hosts: Arc<Store>,
}

#[async_trait]
impl AgentAdmin for TestAgents {
    async fn create_invite(&self, name: &HostName) -> Result<EnrollmentInvite, AgentServerError> {
        let host = match self.hosts.get_host_by_name(name).await? {
            Some(host) => host,
            None => {
                let host = domovoi_core::Host {
                    name: name.clone(),
                    ..host(name.as_str()).unwrap()
                };
                self.hosts.upsert_host(&host).await?;
                host
            }
        };
        Ok(EnrollmentInvite {
            host: host.id,
            token: EnrollToken::new("enroll-token-shown-once"),
            expires_at: t0(),
            fingerprint: "AB:CD".to_owned(),
            command: "domovoi agent enroll ...".to_owned(),
        })
    }

    async fn revoke(&self, host: HostId) -> Result<bool, AgentServerError> {
        Ok(self.hosts.live_agent(host).await?.is_some())
    }

    fn presence(&self, host: HostId) -> Presence {
        domovoi_core::ports::AgentLink::presence(&*self.link, host)
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        domovoi_core::ports::AgentLink::skills(&*self.link, host)
    }

    fn facts(&self, host: HostId) -> Option<HostFacts> {
        self.link.host_facts(host)
    }

    fn checks(&self, host: HostId) -> Vec<CheckInfo> {
        self.link.host_checks(host)
    }

    async fn logs(
        &self,
        _host: HostId,
        _lines: u16,
        _min_level: String,
    ) -> Result<Vec<LogLine>, LinkError> {
        Ok(Vec::new())
    }
}

/// A logged-in browser: the session cookie and CSRF token.
#[derive(Debug, Clone)]
pub struct Login {
    pub cookie: String,
    pub csrf: String,
}

pub struct Api {
    pub app: Router,
    pub state: ApiState,
    pub store: Arc<Store>,
    pub clock: Arc<FakeClock>,
    pub link: Arc<FakeAgentLink>,
    /// The running incident manager, for feeding signals in as the hub's ingest does.
    pub manager: ManagerHandle,
    /// The bus the manager publishes events on, for watching what a signal tells people about.
    pub bus: broadcast::Sender<DomainEvent>,
    /// The model triage asks, when built with [`api_with_triage`].
    pub llm: Arc<ScriptedLlm>,
    /// The audit log the API and the services write to, for reading back what a call recorded.
    pub audit: Arc<dyn AuditLog>,
    /// The use-case layer the API and the chat authorizer call.
    pub services: Services,
    /// The operation service, when built with [`api_with_operations`].
    pub operations: Option<domovoi_engine::operations::OperationHandle>,
    pub incident: Incident,
    pub nas: domovoi_core::Host,
    pub shutdown: CancellationToken,
    pub dir: TempDir,
    /// The incident manager's task, so a test can stop it as a crash would.
    pub manager_task: tokio::task::JoinHandle<()>,
    /// What the policy service's publication review and simulator know of hosts.
    pub policy_env: Arc<crate::policy_env::PolicyFakeEnv>,
    /// The management-path coverage the knowledge service asks; unavailable until a test
    /// calls `cover`.
    pub coverage: Arc<domovoi_testkit::FakeCoverage>,
    /// The capability registry the install service reads: every package contract is available
    /// until a test says otherwise.
    pub package_registry: Arc<domovoi_testkit::FakeCapabilityRegistry>,
    /// What the containment driver's relay answers, and what it was asked.
    pub contain_relay: Arc<domovoi_testkit::FakeContainRelay>,
    /// What the hub holds about each host's helper; a host never set is a host never seen.
    pub helper_trust: Arc<domovoi_testkit::helper_attest::FakeHelperTrust>,
    /// The management paths discovered on hosts, as the containment driver reads them.
    pub contain_paths: Arc<SettablePaths>,
}

/// The discovered management paths a test sets; none until it does.
#[derive(Default)]
pub struct SettablePaths {
    paths: std::sync::Mutex<Vec<domovoi_core::attest::Cidr>>,
}

impl SettablePaths {
    /// Sets the paths every host reports from now on.
    pub fn set(&self, paths: Vec<domovoi_core::attest::Cidr>) {
        *self
            .paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = paths;
    }
}

#[async_trait]
impl domovoi_engine::containment::PathSource for SettablePaths {
    async fn discovered(&self, _host: HostId) -> Result<Vec<domovoi_core::attest::Cidr>, String> {
        Ok(self
            .paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone())
    }
}

/// The variable that locks `triage.concurrency` in the settings tests.
pub const LOCKING_VAR: &str = "DOMOVOI_TRIAGE__CONCURRENCY";

/// The variable that locks `retention.signals_days` in the settings tests.
pub const RETENTION_LOCKING_VAR: &str = "DOMOVOI_RETENTION__SIGNALS_DAYS";

/// The environment of the settings layers: the example's secrets plus overrides.
pub fn settings_env() -> MapEnv {
    let mut env = MapEnv::new()
        .with(LOCKING_VAR, "3")
        .with(RETENTION_LOCKING_VAR, "14");
    for (name, value) in SECRETS {
        env = env.with(name, value);
    }
    env
}

/// The settings service's file layer: `hub.name` and one retention key are locked by the
/// file.
pub const SETTINGS_FILE: &str = "[hub]\nname = \"domovoi\"\n[web]\n\
    public_url = \"https://domovoi.example.net\"\n[retention]\nincident_timeline_days = 45\n";

impl Drop for Api {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// The capability registry over a test's fake link and store.
async fn test_registry(
    link: Arc<FakeAgentLink>,
    store: Arc<Store>,
    clock: Arc<dyn Clock>,
) -> HubRegistry {
    HubRegistry::new(RegistryDeps {
        contracts: Arc::new(ContractIndex::new(known_contracts()).unwrap()),
        agent: Arc::new(domovoi_hub::runtime::caps::LinkCapabilities::new(link)),
        hosts: store.clone(),
        store,
        clock,
    })
    .await
    .unwrap()
}

pub fn config() -> HubConfig {
    let mut env = MapEnv::new();
    for (name, value) in SECRETS {
        env = env.with(name, value);
    }
    let text = include_str!("../../../../examples/hub.full.toml");
    domovoi_config::parse::<HubConfig>(text, &env).unwrap().0
}

pub async fn api() -> Api {
    build(false, None, true, true, TriageSettings::default(), false).await
}

/// [`api`] with `notifier` taking digests.
pub async fn api_with_notifier(notifier: Arc<dyn Notifier>) -> Api {
    build(
        false,
        Some(notifier),
        true,
        true,
        TriageSettings::default(),
        false,
    )
    .await
}

/// [`api_with_notifier`] with the operations service, so steps can wait for approval.
pub async fn api_with_notifier_and_operations(notifier: Arc<dyn Notifier>) -> Api {
    build(
        false,
        Some(notifier),
        true,
        true,
        TriageSettings::default(),
        true,
    )
    .await
}

/// [`api`] with triage on, answering from [`Api::llm`].
pub async fn api_with_triage() -> Api {
    build(true, None, true, true, TriageSettings::default(), false).await
}

/// [`api`] with triage on under `settings`, for the admission refusals.
pub async fn api_with_triage_settings(settings: TriageSettings) -> Api {
    build(true, None, true, true, settings, false).await
}

/// [`api`] whose hub was built without profile collection: the profile endpoints say they
/// are unavailable, and the chat commands that only want the host's line still answer.
pub async fn api_without_profiles() -> Api {
    build(false, None, false, true, TriageSettings::default(), false).await
}

/// [`api`] with nothing captured behind it: the log endpoints say they are unavailable.
pub async fn api_without_logs() -> Api {
    build(false, None, true, false, TriageSettings::default(), false).await
}

/// [`api`] with `[orchestration] enabled` and a running operation service ([`Api::ops`]).
pub async fn api_with_operations() -> Api {
    build(false, None, true, true, TriageSettings::default(), true).await
}

/// [`api_with_operations`] with triage on, so investigations can start.
pub async fn api_with_triage_and_operations() -> Api {
    build(true, None, true, true, TriageSettings::default(), true).await
}

async fn build(
    triage: bool,
    notifier: Option<Arc<dyn Notifier>>,
    profiles: bool,
    logs: bool,
    triage_settings: TriageSettings,
    orchestration: bool,
) -> Api {
    assemble(
        triage,
        notifier,
        profiles,
        logs,
        triage_settings,
        orchestration,
        false,
    )
    .await
}

/// The seed of the key the harness trusts for package imports when built with packages on.
pub const PACKAGE_OWNER_SEED: u8 = 7;

/// The key the harness trusts for package imports when built with packages on.
pub fn package_owner_key() -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[PACKAGE_OWNER_SEED; 32])
}

/// [`api`] with `[packages] enabled` and [`package_owner_key`] as the one owner key.
pub async fn api_with_packages() -> Api {
    assemble(
        false,
        None,
        true,
        true,
        TriageSettings::default(),
        false,
        true,
    )
    .await
}

async fn assemble(
    triage: bool,
    notifier: Option<Arc<dyn Notifier>>,
    profiles: bool,
    logs: bool,
    triage_settings: TriageSettings,
    orchestration: bool,
    packages: bool,
) -> Api {
    // The binary installs the process-wide provider; connection tests build HTTP clients.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("hub.db");
    let store = Arc::new(
        Store::open(&db, StoreOptions::beside(&db, t0()))
            .await
            .unwrap(),
    );
    let clock = Arc::new(FakeClock::new(t0()));
    let link = Arc::new(FakeAgentLink::new());
    let nas = host("nas").unwrap();
    store.upsert_host(&nas).await.unwrap();
    let incident = open_incident(nas.id, "disk.usage", Some("/srv")).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    if triage {
        // The fixture's incident is driven by the test, not by the service's start-up pass:
        // the entry marks it as already triaged, and its empty detail is never read as a
        // report.
        let handled = IncidentTimelineEntry {
            incident: incident.id(),
            at: t0(),
            kind: "triage.cached".to_owned(),
            summary: "the fixture incident is not triaged on start".to_owned(),
            detail: Value::Null,
        };
        IncidentRepo::append_event(&*store, &handled).await.unwrap();
    }

    let (bus, _) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let (manager, manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        uow: store.clone(),
        versions: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let llm = Arc::new(ScriptedLlm::new());
    let triage = triage.then(|| {
        let primary: Arc<dyn LlmProvider> = llm.clone();
        TriageService::spawn(
            TriageDeps {
                primary,
                fallbacks: Vec::new(),
                evidence: domovoi_engine::Evidence::new(
                    evidence_caps(link.clone()),
                    clock.clone(),
                    Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
                        link.clone(),
                    )),
                ),
                catalog: domovoi_engine::HostCatalog::new(observer_of(link.clone())),
                capabilities: Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
                    link.clone(),
                )),
                resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
                incidents: store.clone(),
                acks: store.clone(),
                signals: store.clone(),
                notes: store.clone(),
                settings: store.clone(),
                hosts: store.clone(),
                cache: store.clone(),
                usage: store.clone(),
                admission: store.clone(),
                advisories: store.clone(),
                redactor: Arc::new(PatternRedactor::default()),
                clock: clock.clone(),
                manager: manager.clone(),
                bus: bus.clone(),
                shutdown: shutdown.clone(),
                experience: domovoi_engine::context::ExperienceSource::over(
                    store.clone(),
                    Arc::new(PatternRedactor::default()),
                    clock.clone(),
                ),
                extra_context: Vec::new(),
            },
            triage_settings,
        )
        .0
    });
    let mut config = config();
    config.orchestration.enabled = orchestration;
    config.packages.enabled = packages;
    if packages {
        config.packages.max_artifact_bytes = 64 * 1024;
        let public = package_owner_key().verifying_key().to_bytes();
        config.packages.owner_keys = vec![domovoi_config::TrustedKey {
            id: domovoi_core::package::KeyId::of_public_key(&public).to_string(),
            public_key: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, public),
            label: Some("Owner".to_owned()),
        }];
    }
    let contain_relay = Arc::new(domovoi_testkit::FakeContainRelay::new());
    let helper_trust = Arc::new(domovoi_testkit::helper_attest::FakeHelperTrust::new());
    let paths = Arc::new(SettablePaths::default());
    let containment = domovoi_engine::containment::ContainmentDriver::new(
        domovoi_engine::containment::ContainDeps {
            caps: domovoi_testkit::contain::contain_caps(link.clone(), contain_relay.clone()),
            uow: store.clone(),
            blocks: store.clone(),
            trust: helper_trust.clone(),
            paths: paths.clone(),
            resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
            clock: clock.clone(),
            result_grace: SignedDuration::from_secs(30),
        },
    );
    let operations = if orchestration {
        Some(
            crate::ops_harness::spawn(
                &store,
                &link,
                &clock,
                &shutdown,
                &nas,
                triage.clone(),
                Some(containment.clone()),
            )
            .await,
        )
    } else {
        None
    };
    let web = WebIdentity {
        public_url: config.web.public_url.clone().unwrap(),
        trusted_proxies: config.web.trusted_proxies.clone(),
    };
    let auth = Arc::new(AuthState::new(
        store.clone(),
        store.clone(),
        clock.clone(),
        web,
    ));
    let mut server_settings = ServerSettings::from_config(&config).unwrap();
    server_settings.data_dir = dir.path().to_path_buf();
    let server = AgentServer::new(
        server_settings,
        AgentServerDeps {
            clock: clock.clone(),
            hosts: store.clone(),
            facts: store.clone(),
            sink: Arc::new(RecordingSignalSink::new()),
            audit: store.clone(),
            probes: Arc::new(RealNetProbes::new(clock.clone())),
            hub_host: nas.id,
        },
    )
    .unwrap();
    server.use_check_overrides(store.clone());
    let settings = SettingsService::new(SettingsDeps {
        store: store.clone(),
        key: Arc::new(SecretKey::load_or_create(dir.path()).unwrap()),
        sources: ConfigSources {
            file: Some({
                let path = dir.path().join("hub.toml");
                std::fs::write(&path, SETTINGS_FILE).unwrap();
                path
            }),
            env: Arc::new(settings_env()),
        },
        clock: clock.clone(),
        auth: auth.clone(),
        agents: server.clone(),
        apply: None,
        executor: None,
    });
    let source: Arc<dyn LogSource> = Arc::new(CannedLogSource::new(canned_logs()));
    let update = UpdateChecker::load(store.clone(), clock.clone()).await;
    let agents: Arc<dyn AgentAdmin> = Arc::new(TestAgents {
        link: link.clone(),
        hosts: store.clone(),
    });
    let profile_deps = profiles.then(|| ProfileDeps {
        evidence: domovoi_engine::Evidence::new(
            evidence_caps(link.clone()),
            clock.clone(),
            Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
                link.clone(),
            )),
        ),
        catalog: domovoi_engine::HostCatalog::new(observer_of(link.clone())),
        capabilities: Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
            link.clone(),
        )),
        agents: agents.clone(),
        hosts: store.clone(),
        notes: store.clone(),
        settings: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        knowledge: None,
    });
    let config = Arc::new(config);
    let policy_env = Arc::new(crate::policy_env::PolicyFakeEnv::new());
    let policy = PolicyService::new(PolicyDeps {
        repo: store.clone(),
        uow: store.clone(),
        env: policy_env.clone(),
        auth: auth.clone(),
        clock: clock.clone(),
        shadow_enabled: true,
    })
    .with_autonomy(domovoi_hub::services::AutonomyDeps {
        permits: store.clone(),
        operations: store.clone(),
        circuit_failures: 2,
        circuit_window: jiff::SignedDuration::from_hours(24),
    });
    let coverage = Arc::new(domovoi_testkit::FakeCoverage::unavailable());
    let knowledge = Arc::new(domovoi_engine::knowledge::KnowledgeService::new(
        domovoi_engine::knowledge::KnowledgeDeps {
            uow: store.clone(),
            read: store.clone(),
            coverage: coverage.clone(),
            clock: clock.clone(),
        },
    ));
    let live = domovoi_hub::api::operator::live::LiveBus::new();
    let chat_model: Arc<dyn domovoi_engine::conversations::ChatModel> = match triage.clone() {
        Some(handle) => Arc::new(handle),
        None => Arc::new(domovoi_engine::conversations::DisabledModel),
    };
    let (conversation_handle, _conversation_task) =
        domovoi_engine::conversations::ConversationService::spawn(
            domovoi_engine::conversations::ConversationDeps {
                conversations: store.clone(),
                uow: store.clone(),
                model: chat_model,
                live: live.clone(),
                redactor: Arc::new(PatternRedactor::default()),
                clock: clock.clone(),
                shutdown: shutdown.clone(),
            },
        )
        .await
        .unwrap();
    let package_registry = Arc::new(domovoi_testkit::FakeCapabilityRegistry::new(Arc::new(
        ContractIndex::new(domovoi_core::capability::known_contracts()).unwrap(),
    )));
    let package_files = domovoi_hub::packages::PackageStore::open(dir.path())
        .await
        .unwrap();
    let package_importer = Arc::new(domovoi_hub::packages::PackageImporter::new(
        config.packages.clone(),
        Arc::new(domovoi_hub::packages::PackageKeys::from_config(&config.packages).unwrap()),
        package_files.clone(),
        store.clone(),
        store.clone(),
        clock.clone(),
    ));
    let (package_handle, _package_task) = domovoi_engine::packages::InstallService::spawn(
        domovoi_engine::packages::PackageDeps {
            installs: store.clone(),
            uow: store.clone(),
            hosts: store.clone(),
            resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
            registry: package_registry.clone(),
            caps: domovoi_hub::runtime::caps::package_caps(link.clone()),
            files: Arc::new(package_files),
            clock: clock.clone(),
            shutdown: shutdown.clone(),
        },
        domovoi_engine::packages::PackageSettings::default(),
    );
    let services = Services::new(ServicesDeps {
        manager: manager.clone(),
        triage: triage.clone(),
        operations: operations.clone(),
        operation_repo: store.clone(),
        incidents: store.clone(),
        hosts: store.clone(),
        facts: store.clone(),
        advisories: store.clone(),
        agents: agents.clone(),
        profiles: profile_deps.clone(),
        config: config.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy,
        knowledge: domovoi_hub::services::KnowledgeCommands::new(
            knowledge,
            store.clone(),
            auth.clone(),
        ),
        conversations: domovoi_hub::services::ConversationCommands::new(conversation_handle),
        settings: store.clone(),
        helper_trust: domovoi_hub::services::HelperTrust::new(
            domovoi_hub::services::HelperTrustDeps {
                hub: Arc::new(domovoi_hub::helper_trust::HelperTrustHub::new(
                    store.clone(),
                    store.clone(),
                    link.clone(),
                    clock.clone(),
                )),
                hosts: store.clone(),
                auth: auth.clone(),
                clock: clock.clone(),
            },
        ),
        containment: domovoi_hub::services::ContainmentDeps {
            driver: Some(containment),
            repo: store.clone(),
            hosts: store.clone(),
            auth: auth.clone(),
            clock: clock.clone(),
        },
        packages: domovoi_hub::services::PackageCommands::new(domovoi_hub::services::PackageDeps {
            handle: package_handle,
            importer: package_importer,
            installs: store.clone(),
            hosts: store.clone(),
            auth: auth.clone(),
            clock: clock.clone(),
        }),
    });
    let deps = ApiDeps {
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
        triage,
        operation_repo: store.clone(),
        conversations: store.clone(),
        live,
        experience: store.clone(),
        policy: store.clone(),
        agents,
        capabilities: Arc::new(test_registry(link.clone(), store.clone(), clock.clone()).await),
        logs: if logs { Some(source) } else { None },
        redactor: Arc::new(PatternRedactor::default()),
        profiles: profile_deps,
        clock: clock.clone(),
        journal: store.clone(),
        config,
        notifier,
        metrics: Some(Arc::new(|| "domovoi_up 1\n".to_owned())),
        store_health: store.clone(),
        settings: Some(settings),
        audit: store.clone(),
        audit_inspect: store.clone(),
        update,
    };
    let state = ApiState::new(deps, auth);
    let app = router(state.clone(), None).layer(MockConnectInfo(peer()));
    Api {
        audit: store.clone(),
        services,
        app,
        state,
        store,
        clock,
        link,
        manager,
        bus,
        llm,
        incident,
        nas,
        operations,
        shutdown,
        dir,
        manager_task,
        policy_env,
        coverage,
        package_registry,
        contain_relay,
        helper_trust,
        contain_paths: paths,
    }
}

/// Reads a response into a [`Reply`].
async fn reply_of(response: axum::response::Response) -> Reply {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Reply {
        status,
        headers,
        body: serde_json::from_str(&text).unwrap_or(Value::Null),
        text,
    }
}

/// A response's status, headers and JSON body (`Null` when not JSON).
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
    pub text: String,
}

impl Api {
    pub fn now(&self) -> Timestamp {
        domovoi_core::ports::Clock::now(&*self.clock)
    }

    pub fn advance(&self, by: SignedDuration) {
        self.clock.advance(by);
    }

    /// Sends `method uri` with `body`, as `login` when given (cookie and CSRF token).
    pub async fn send(
        &self,
        method: Method,
        uri: &str,
        login: Option<&Login>,
        body: Option<Value>,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(login) = login {
            req = req
                .header(header::COOKIE, &login.cookie)
                .header("x-csrf-token", &login.csrf);
        }
        let req = match body {
            Some(body) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => req.body(Body::empty()),
        };
        self.raw(req.unwrap()).await
    }

    /// Sends a request through `app` instead of the harness's own router.
    pub async fn send_to(
        &self,
        app: Router,
        method: Method,
        uri: &str,
        login: Option<&Login>,
        body: Option<Value>,
    ) -> Reply {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(login) = login {
            req = req
                .header(header::COOKIE, &login.cookie)
                .header("x-csrf-token", &login.csrf);
        }
        let req = match body {
            Some(body) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => req.body(Body::empty()),
        };
        let response = app.oneshot(req.unwrap()).await.unwrap();
        reply_of(response).await
    }

    pub async fn raw(&self, req: Request<Body>) -> Reply {
        reply_of(self.app.clone().oneshot(req).await.unwrap()).await
    }

    pub async fn get(&self, uri: &str, login: &Login) -> Reply {
        self.send(Method::GET, uri, Some(login), None).await
    }

    pub async fn post(&self, uri: &str, login: &Login, body: Value) -> Reply {
        self.send(Method::POST, uri, Some(login), Some(body)).await
    }

    /// The app over the same state with `health` answering `/readyz`.
    pub fn with_store_health(&self, health: Arc<dyn StoreHealth>) -> Router {
        let mut deps: ApiDeps = (**self.state.deps()).clone();
        deps.store_health = health;
        let state =
            ApiState::new(deps, self.state.auth().clone()).with_shutdown(self.shutdown.clone());
        router(state, None).layer(MockConnectInfo(peer()))
    }

    /// The app over the same state with `experience` answering the summary reads.
    pub fn with_experience(
        &self,
        experience: Arc<dyn domovoi_core::ports::ExperienceRead>,
    ) -> Router {
        let mut deps: ApiDeps = (**self.state.deps()).clone();
        deps.experience = experience;
        let state =
            ApiState::new(deps, self.state.auth().clone()).with_shutdown(self.shutdown.clone());
        router(state, None).layer(MockConnectInfo(peer()))
    }

    /// Stops the incident manager and waits for its task to end, as a crashed service
    /// would: its queue closes, so `ManagerHandle::is_alive` is false.
    pub async fn stop_manager(&self) {
        self.manager_task.abort();
        let _ = self.manager_task.is_finished();
        tokio::task::yield_now().await;
    }

    /// The audit events of `kind`, newest first: what a write recorded.
    pub async fn audit_events(&self, kind: &str) -> Vec<AuditEvent> {
        self.audit
            .list(
                &AuditFilter {
                    kind: Some(kind.to_owned()),
                    ..AuditFilter::default()
                },
                &Page::first(100),
            )
            .await
            .unwrap()
            .items
    }

    /// The setup token from the link the hub would log.
    pub async fn setup_token(&self) -> String {
        let link = self.state.auth().open_setup().await.unwrap().unwrap();
        link.split_once("token=").unwrap().1.to_owned()
    }

    /// Creates the admin through the setup endpoint and returns the logged-in session.
    pub async fn admin(&self) -> Login {
        let token = self.setup_token().await;
        let reply = self
            .send(
                Method::POST,
                "/api/v1/auth/setup",
                None,
                Some(serde_json::json!({ "token": token, "username": USERNAME, "password": PASSWORD })),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        login_of(&reply)
    }

    pub async fn login(&self, password: &str) -> Reply {
        self.send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(serde_json::json!({ "username": USERNAME, "password": password })),
        )
        .await
    }
}

/// The session in a login or setup reply.
pub fn login_of(reply: &Reply) -> Login {
    let set_cookie = reply.headers[header::SET_COOKIE].to_str().unwrap();
    let cookie = set_cookie.split(';').next().unwrap().to_owned();
    Login {
        cookie,
        csrf: reply.body["csrf_token"].as_str().unwrap().to_owned(),
    }
}
