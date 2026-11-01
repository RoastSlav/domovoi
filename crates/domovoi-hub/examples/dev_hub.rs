//! A throwaway hub for working on the web UI: the real router over a temporary SQLite store,
//! a running incident manager and action service, and fake agents for two hosts.
//!
//! ```sh
//! cargo run -p domovoi-hub --example dev_hub --features embed-ui
//! ```
//!
//! It serves on 127.0.0.1:7420, prints the setup link, and seeds an incident with a
//! proposed write action and a destructive one. Press Enter to fire another incident and
//! watch it arrive live. Without `embed-ui`, run `npm run dev` in `web/` against it.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use domovoi_config::{HubConfig, MapEnv};
use domovoi_core::ports::{
    ActionRepo, Clock, HostRepo, IncidentFilter, IncidentRepo, Page, Presence, SkillOutput,
};
use domovoi_core::{
    ActionRequest, CheckKey, Host, HostId, HostKind, HostName, Principal, Risk, Severity, Signal,
    SignalClass, SkillSummary, SourceKind, TargetKey,
};
use domovoi_engine::{
    ActionDeps, ActionService, ActionSettings, EnginePolicy, IncidentManager, ManagerDeps,
    ManagerHandle, PatternRedactor,
};
use domovoi_hub::agent_server::{AgentServerError, EnrollmentInvite};
use domovoi_hub::api::{AgentAdmin, ApiDeps, ApiState};
use domovoi_hub::auth::{AuthState, WebIdentity};
use domovoi_hub::router::router;
use domovoi_proto::EnrollToken;
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::FakeAgentLink;
use domovoi_testkit::build::{host, manifest};
use jiff::{SignedDuration, Timestamp};
use tokio::io::AsyncBufReadExt;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use url::Url;

const ADDR: &str = "127.0.0.1:7420";

/// Prints one line for the person running the example.
fn say(line: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// The wall clock; the hub runtime brings its own.
struct WallClock;

#[async_trait]
impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    async fn sleep_until(&self, t: Timestamp) {
        let wait = Timestamp::now().duration_until(t);
        if let Ok(wait) = Duration::try_from(wait) {
            tokio::time::sleep(wait).await;
        }
    }
}

/// Fake agents: presence and catalogues from the fake link, invites that cannot enroll.
struct DevAgents {
    link: Arc<FakeAgentLink>,
    store: Arc<Store>,
}

#[async_trait]
impl AgentAdmin for DevAgents {
    async fn create_invite(
        &self,
        name: &HostName,
    ) -> std::result::Result<EnrollmentInvite, AgentServerError> {
        let host = match self.store.get_host_by_name(name).await? {
            Some(host) => host,
            None => {
                let new = Host {
                    id: HostId::new(),
                    name: name.clone(),
                    kind: HostKind::Remote,
                    labels: BTreeMap::new(),
                    created_at: Timestamp::now(),
                };
                self.store.upsert_host(&new).await?;
                new
            }
        };
        Ok(EnrollmentInvite {
            host: host.id,
            token: EnrollToken::new("dev-hub-token-enrolls-nothing"),
            expires_at: Timestamp::now(),
            fingerprint: "dev".to_owned(),
            command: "domovoi agent enroll (dev hub: no agent server)".to_owned(),
        })
    }

    async fn revoke(&self, _host: HostId) -> std::result::Result<bool, AgentServerError> {
        Ok(false)
    }

    fn presence(&self, host: HostId) -> Presence {
        domovoi_core::ports::AgentLink::presence(&*self.link, host)
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        domovoi_core::ports::AgentLink::skills(&*self.link, host)
    }
}

fn config() -> Result<HubConfig> {
    let env = [
        "ANTHROPIC_API_KEY",
        "DISCORD_BOT_TOKEN",
        "DISCORD_WEBHOOK_URL",
        "DOMOVOI_ALERTS_TOKEN",
        "DOMOVOI_HEARTBEAT_URL",
    ]
    .into_iter()
    .fold(MapEnv::new(), |env, name| {
        env.with(name, "https://dev.invalid/secret")
    });
    let text = include_str!("../../../examples/hub.toml");
    domovoi_config::parse::<HubConfig>(text, &env)
        .map(|(config, _)| config)
        .map_err(|issues| format!("examples/hub.toml: {issues:?}").into())
}

fn firing(
    host: HostId,
    check: &str,
    target: &str,
    severity: Severity,
    summary: &str,
) -> Result<Signal> {
    Ok(Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse(check)?,
        Some(TargetKey::parse(target)?),
        severity,
        summary,
        Timestamp::now(),
    )
    .with_class(SignalClass::Immediate))
}

async fn seed_actions(store: &Store, nas: HostId) -> Result<()> {
    let mut found = None;
    for _ in 0..50 {
        let page = IncidentRepo::list(
            store,
            &IncidentFilter::default(),
            &Page {
                cursor: None,
                limit: 1,
            },
        )
        .await?;
        if let Some(incident) = page.items.into_iter().next() {
            found = Some(incident);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let Some(incident) = found else {
        return Err("the seeded incident never opened".into());
    };
    for (skill, risk) in [
        ("restart_container", Risk::Write),
        ("delete_volume", Risk::Destructive),
    ] {
        let summary = manifest(skill, risk)?.summary();
        let args = summary.validate_args(&BTreeMap::new())?;
        let action = ActionRequest::propose(
            incident.id(),
            nas,
            &summary,
            &args,
            Principal::System("triage".into()),
            SignedDuration::from_hours(4),
            Timestamp::now(),
        );
        ActionRepo::insert(store, &action).await?;
    }
    Ok(())
}

async fn fire_on_enter(manager: ManagerHandle, pi: HostId) -> Result<()> {
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut n = 0u32;
    while lines.next_line().await?.is_some() {
        n += 1;
        let signal = firing(
            pi,
            "service.down",
            &format!("worker-{n}"),
            Severity::Critical,
            &format!("worker-{n} stopped"),
        )?;
        manager.ingest(vec![signal]).await?;
        say(format_args!("fired incident worker-{n}"));
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("hub.db");
    let store = Arc::new(Store::open(&db, StoreOptions::beside(&db, Timestamp::now())).await?);
    let clock: Arc<dyn Clock> = Arc::new(WallClock);
    let link = Arc::new(FakeAgentLink::new());
    link.on_skill(|_, _| {
        Ok(SkillOutput {
            exit_code: Some(0),
            stdout: "done\n".to_owned(),
            stderr: String::new(),
            truncated: false,
            duration_ms: 120,
            argv_hash: [0; 32],
        })
    });

    let mut hosts = Vec::new();
    for name in ["nas", "pi"] {
        let mut new = host(name)?;
        new.created_at = Timestamp::now();
        store.upsert_host(&new).await?;
        hosts.push(new.id);
    }
    let [nas, pi] = hosts[..] else {
        return Err("two hosts expected".into());
    };
    let skills = [
        ("restart_container", Risk::Write),
        ("delete_volume", Risk::Destructive),
    ]
    .into_iter()
    .map(|(skill, risk)| manifest(skill, risk).map(|m| m.summary()))
    .collect::<std::result::Result<Vec<_>, _>>()?;
    link.set_skills(nas, skills);
    link.set_presence(
        nas,
        Presence::Connected {
            since: Timestamp::now(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    );

    let (bus, _) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let (actions_service, _actions_task) = ActionService::spawn(
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

    let config = config()?;
    let web = WebIdentity {
        public_url: Url::parse(&format!("http://{ADDR}"))?,
        trusted_proxies: Vec::new(),
    };
    let auth = Arc::new(AuthState::new(
        store.clone(),
        store.clone(),
        clock.clone(),
        web,
    ));
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
        triage: None,
        agents: Arc::new(DevAgents {
            link: link.clone(),
            store: store.clone(),
        }),
        clock: clock.clone(),
        bus,
        config: Arc::new(config),
        notifier: None,
        metrics: None,
    };
    let state = ApiState::new(deps, auth.clone());
    let app = router(state, None).into_make_service_with_connect_info::<SocketAddr>();

    manager
        .ingest(vec![firing(
            nas,
            "disk.usage",
            "/srv",
            Severity::Warning,
            "Disk /srv is 93% full",
        )?])
        .await?;
    seed_actions(&store, nas).await?;

    let listener = tokio::net::TcpListener::bind(ADDR).await?;
    if let Some(link) = auth.open_setup().await? {
        say(format_args!("Open {link}"));
    }
    say(format_args!(
        "Serving on http://{ADDR}. Press Enter to fire an incident on pi; Ctrl+C to stop."
    ));
    tokio::spawn(fire_on_enter(manager, pi));
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.cancel();
        })
        .await?;
    Ok(())
}
