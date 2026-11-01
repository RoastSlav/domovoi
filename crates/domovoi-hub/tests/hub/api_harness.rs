//! The hub API over a temporary SQLite store, a running incident manager and action
//! service, fakes for agents and time, and the example `hub.toml` with test secrets.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use domovoi_config::{HubConfig, MapEnv};
use domovoi_core::ports::{ActionRepo, HostRepo, IncidentRepo, LlmProvider, Presence};
use domovoi_core::{ActionRequest, HostId, HostName, Incident, Principal, Risk, SkillSummary};
use domovoi_engine::{
    ActionDeps, ActionService, ActionSettings, EnginePolicy, IncidentManager, ManagerDeps,
    ManagerHandle, PatternRedactor, TriageDeps, TriageService, TriageSettings,
};
use domovoi_hub::agent_server::{
    AgentServer, AgentServerDeps, AgentServerError, EnrollmentInvite, RealNetProbes, ServerSettings,
};
use domovoi_hub::api::{AgentAdmin, ApiDeps, ApiState};
use domovoi_hub::auth::{AuthState, WebIdentity};
use domovoi_hub::router::router;
use domovoi_hub::settings::{ConfigSources, SecretKey, SettingsDeps, SettingsService};
use domovoi_proto::EnrollToken;
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{host, manifest, open_incident};
use domovoi_testkit::{FakeAgentLink, FakeClock, RecordingSignalSink, ScriptedLlm};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// The origin of the example config's `public_url`.
pub const ORIGIN: &str = "https://domovoi.example.net";

/// Secret values the example config reads; none may ever appear in a response.
pub const SECRETS: [(&str, &str); 5] = [
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
    /// The model triage asks, when built with [`api_with_triage`].
    pub llm: Arc<ScriptedLlm>,
    pub incident: Incident,
    pub nas: domovoi_core::Host,
    pub shutdown: CancellationToken,
    pub dir: TempDir,
}

/// The variable that locks `triage.concurrency` in the settings tests.
pub const LOCKING_VAR: &str = "DOMOVOI_TRIAGE__CONCURRENCY";

/// The environment of the settings layers: the example's secrets plus one override.
pub fn settings_env() -> MapEnv {
    let mut env = MapEnv::new().with(LOCKING_VAR, "3");
    for (name, value) in SECRETS {
        env = env.with(name, value);
    }
    env
}

/// The settings service's file layer: `hub.name` is locked by the file.
pub const SETTINGS_FILE: &str =
    "[hub]\nname = \"domovoi\"\n[web]\npublic_url = \"https://domovoi.example.net\"\n";

impl Drop for Api {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
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
    build(false).await
}

/// [`api`] with triage on, answering from [`Api::llm`].
pub async fn api_with_triage() -> Api {
    build(true).await
}

async fn build(triage: bool) -> Api {
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

    let (bus, _) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let (manager, _task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let (actions_service, _task) = ActionService::spawn(
        ActionDeps {
            actions: store.clone(),
            incidents: store.clone(),
            audit: store.clone(),
            link: link.clone(),
            redactor: Arc::new(PatternRedactor::default()),
            clock: clock.clone(),
            manager: manager.clone(),
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        },
        ActionSettings::default(),
    );
    let llm = Arc::new(ScriptedLlm::new());
    let triage = triage.then(|| {
        let primary: Arc<dyn LlmProvider> = llm.clone();
        TriageService::spawn(
            TriageDeps {
                primary,
                secondary: None,
                link: link.clone(),
                incidents: store.clone(),
                signals: store.clone(),
                notes: store.clone(),
                actions: store.clone(),
                audit: store.clone(),
                cache: store.clone(),
                usage: store.clone(),
                advisories: store.clone(),
                redactor: Arc::new(PatternRedactor::default()),
                clock: clock.clone(),
                manager: manager.clone(),
                bus: bus.clone(),
                shutdown: shutdown.clone(),
            },
            TriageSettings::default(),
        )
        .0
    });
    let config = config();
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
    let deps = ApiDeps {
        incidents: store.clone(),
        signals: store.clone(),
        actions: store.clone(),
        hosts: store.clone(),
        notes: store.clone(),
        advisories: store.clone(),
        facts: store.clone(),
        usage: store.clone(),
        manager: manager.clone(),
        actions_service,
        triage,
        agents: Arc::new(TestAgents {
            link: link.clone(),
            hosts: store.clone(),
        }),
        clock: clock.clone(),
        bus: bus.clone(),
        config: Arc::new(config),
        notifier: None,
        metrics: Some(Arc::new(|| "domovoi_up 1\n".to_owned())),
        settings: Some(settings),
        audit: store.clone(),
    };
    let state = ApiState::new(deps, auth);
    let app = router(state.clone(), None).layer(MockConnectInfo(peer()));
    Api {
        app,
        state,
        store,
        clock,
        link,
        manager,
        llm,
        incident,
        nas,
        shutdown,
        dir,
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

    pub async fn raw(&self, req: Request<Body>) -> Reply {
        let response = self.app.clone().oneshot(req).await.unwrap();
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

    /// Proposes `restart_container` (no parameters) with `risk`, open for an hour.
    pub async fn propose(&self, risk: Risk) -> ActionRequest {
        let summary = manifest("restart_container", risk).unwrap().summary();
        let args = summary.validate_args(&BTreeMap::new()).unwrap();
        let action = ActionRequest::propose(
            self.incident.id(),
            self.nas.id,
            &summary,
            &args,
            Principal::System("triage".into()),
            SignedDuration::from_hours(1),
            self.now(),
        );
        ActionRepo::insert(&*self.store, &action).await.unwrap();
        action
    }

    pub async fn get(&self, uri: &str, login: &Login) -> Reply {
        self.send(Method::GET, uri, Some(login), None).await
    }

    pub async fn post(&self, uri: &str, login: &Login, body: Value) -> Reply {
        self.send(Method::POST, uri, Some(login), Some(body)).await
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
