//! A throwaway hub for working on the web UI: the real router over a temporary SQLite store,
//! a running incident manager and action service, and fake agents for two hosts.
//!
//! ```sh
//! cargo run -p domovoi-hub --example dev_hub --features embed-ui
//! ```
//!
//! It serves on 127.0.0.1:7420, prints the setup link, and seeds an incident with a
//! proposed write action and a destructive one, plus a host profile for `nas`. Press Enter
//! to fire another incident and
//! watch it arrive live. Without `embed-ui`, run `npm run dev` in `web/` against it.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use domovoi_config::{HubConfig, MapEnv};
use domovoi_core::ports::{
    ActionRepo, Clock, HostRepo, IncidentFilter, IncidentRepo, LinkError, LogLine, Page, Presence,
    SkillOutput, ToolOutput,
};
use domovoi_core::{
    ActionRequest, CheckKey, HostId, HostName, Principal, ReadToolCall, Risk, Severity, Signal,
    SignalClass, SkillSummary, SourceKind, TargetKey,
};
use domovoi_engine::{
    ActionDeps, ActionService, ActionSettings, EnginePolicy, IncidentManager, ManagerDeps,
    ManagerHandle, PatternRedactor,
};
use domovoi_hub::agent_server::{
    AgentServer, AgentServerDeps, AgentServerError, EnrollmentInvite, RealNetProbes,
    ServerSettings, SkillPolicy,
};
use domovoi_hub::api::{AgentAdmin, ApiDeps, ApiState};
use domovoi_hub::auth::{AuthState, WebIdentity};
use domovoi_hub::profile::ProfileDeps;
use domovoi_hub::router::router;
use domovoi_hub::runtime::updates::UpdateChecker;
use domovoi_proto::messages::{CheckInfo, HostFacts};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{host, manifest};
use domovoi_testkit::{CannedLogSource, FakeAgentLink};
use jiff::{SignedDuration, Timestamp};
use tokio::io::AsyncBufReadExt;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use url::Url;

const ADDR: &str = "127.0.0.1:7420";

/// Where agents connect.
const AGENT_ADDR: &str = "127.0.0.1:7421";

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

/// Agents for the dev hub: real enrollment through the agent server, with the seeded
/// hosts' presence and skills from the fake link until a real agent connects.
struct DevAgents {
    server: AgentServer,
    link: Arc<FakeAgentLink>,
}

#[async_trait]
impl AgentAdmin for DevAgents {
    async fn create_invite(
        &self,
        name: &HostName,
    ) -> std::result::Result<EnrollmentInvite, AgentServerError> {
        self.server.create_token(name).await
    }

    async fn revoke(&self, host: HostId) -> std::result::Result<bool, AgentServerError> {
        self.server.revoke_agent(host).await
    }

    fn presence(&self, host: HostId) -> Presence {
        match AgentAdmin::presence(&self.server, host) {
            connected @ Presence::Connected { .. } => connected,
            Presence::Disconnected { .. } => {
                domovoi_core::ports::AgentLink::presence(&*self.link, host)
            }
        }
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        let real = AgentAdmin::skills(&self.server, host);
        if real.is_empty() {
            domovoi_core::ports::AgentLink::skills(&*self.link, host)
        } else {
            real
        }
    }

    fn facts(&self, host: HostId) -> Option<HostFacts> {
        match AgentAdmin::facts(&self.server, host) {
            Some(facts) => Some(facts),
            None => self.link.host_facts(host),
        }
    }

    fn checks(&self, host: HostId) -> Vec<CheckInfo> {
        let real = AgentAdmin::checks(&self.server, host);
        if real.is_empty() {
            self.link.host_checks(host)
        } else {
            real
        }
    }

    async fn logs(
        &self,
        host: HostId,
        lines: u16,
        min_level: String,
    ) -> std::result::Result<Vec<LogLine>, LinkError> {
        AgentAdmin::logs(&self.server, host, lines, min_level).await
    }
}

/// The agent server on [`AGENT_ADDR`], keeping its TLS identity under `data_dir`.
fn agent_server(
    data_dir: &std::path::Path,
    clock: Arc<dyn Clock>,
    store: Arc<Store>,
    sink: ManagerHandle,
) -> Result<AgentServer> {
    let settings = ServerSettings {
        hub_name: "dev-hub".to_owned(),
        data_dir: data_dir.to_path_buf(),
        bind: AGENT_ADDR.parse()?,
        public_url: Url::parse(&format!("wss://{AGENT_ADDR}"))?,
        self_check: "1.1.1.1:53".to_owned(),
        heartbeat: SignedDuration::from_secs(15),
        probes: BTreeMap::new(),
    };
    let deps = AgentServerDeps {
        clock: clock.clone(),
        hosts: store.clone(),
        facts: store.clone(),
        sink: Arc::new(sink),
        audit: store,
        probes: Arc::new(RealNetProbes::new(clock)),
        hub_host: HostId::new(),
    };
    Ok(AgentServer::new(settings, deps)?)
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
    let text = include_str!("../../../examples/hub.full.toml");
    domovoi_config::parse::<HubConfig>(text, &env)
        .map(|(config, _)| config)
        .map_err(|issues| format!("examples/hub.full.toml: {issues:?}").into())
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

/// What each press of Enter fires, in turn: different hosts, checks and severities, and one
/// long title, so a busy incident list can be looked at.
const FIRES: [(bool, &str, &str, Severity, &str); 8] = [
    (
        false,
        "service.down",
        "worker",
        Severity::Critical,
        "stopped",
    ),
    (true, "disk.usage", "/var", Severity::Warning, "is 88% full"),
    (
        false,
        "container.restarts",
        "jellyfin",
        Severity::Warning,
        "restarted 5 times in 10 minutes",
    ),
    (
        true,
        "smart.health",
        "sda",
        Severity::Critical,
        "reports reallocated sectors",
    ),
    (
        false,
        "cert.expiry",
        "home.example.net",
        Severity::Info,
        "expires in 12 days",
    ),
    (
        true,
        "memory.available",
        "host",
        Severity::Warning,
        "below 5% for 15 minutes",
    ),
    (
        false,
        "backup.age",
        "nightly-offsite-backup-to-the-storage-box",
        Severity::Critical,
        "has not completed for 3 days and the last attempt failed with a timeout",
    ),
    (true, "temperature", "cpu", Severity::Info, "at 78 °C"),
];

async fn fire_on_enter(manager: ManagerHandle, pi: HostId, nas: HostId) -> Result<()> {
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut n = 0usize;
    while lines.next_line().await?.is_some() {
        let (on_nas, check, target, severity, what) = FIRES[n % FIRES.len()];
        n += 1;
        let target = format!("{target}-{n}");
        let signal = firing(
            if on_nas { nas } else { pi },
            check,
            &target,
            severity,
            &format!("{target} {what}"),
        )?;
        manager.ingest(vec![signal]).await?;
        say(format_args!("fired incident {target}"));
    }
    Ok(())
}

/// Log lines the dev hub answers with, so the web UI's Logs page has something to show.
fn dev_logs() -> Vec<LogLine> {
    let at = Timestamp::now();
    [
        LogLine {
            at,
            level: "info".to_owned(),
            target: "domovoi_hub.runtime".to_owned(),
            message: "hub started hosts=2".to_owned(),
        },
        LogLine {
            at,
            level: "warn".to_owned(),
            target: "domovoi_hub.checks".to_owned(),
            message: "disk.usage on /srv is 91% full".to_owned(),
        },
        LogLine {
            at,
            level: "error".to_owned(),
            target: "domovoi_hub.chat".to_owned(),
            message: "gateway rejected key=sk-ant-dev-0000111122223333".to_owned(),
        },
    ]
    .to_vec()
}

#[tokio::main]
async fn main() -> Result<()> {
    // The binary installs the process-wide provider; here nothing else does, and the
    // release checker builds an HTTP client.
    let _ = rustls::crypto::ring::default_provider().install_default();
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
    link.on_tool(|_, call| {
        let text = match call {
            ReadToolCall::HostOverview => ("uptime: 41 days\nload: 0.08 0.11 0.09\nmemory: 31 GiB total, 6.2 GiB used\ndisks:\n  /srv 2.0T 91%").to_owned(),
            ReadToolCall::DockerPs { .. } => ("name\timage\tstate\thealth\trestarts\nnfsd\tnfs\tstarted\thealthy\t0\ndata\tpolardb\tstarted\tdegraded\t3").to_owned(),
            ReadToolCall::ListeningPorts => ("2049 ephemeris (nfs)\n8101 samba (LISTEN) fd 12 (*:8101)").to_owned(),
            _ => ("Nov 04 05:12 systemd[1]: Restarted job.").to_owned(),
        };
        Ok(ToolOutput { text, truncated: false })
    });
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
    link.set_facts(
        nas,
        HostFacts {
            hostname: "diskstation".to_owned(),
            os: "Debian GNU/Linux 13".to_owned(),
            kernel: "6.12.9".to_owned(),
            arch: "x86_64".to_owned(),
            cpu_count: 4,
            mem_total_bytes: 34_359_738_368,
            boot_time: Timestamp::now(),
            boot_id: "8f2c1e4a6b0d".to_owned(),
        },
    );
    link.set_checks(
        nas,
        [
            CheckInfo {
                key: CheckKey::parse("disk.usage")?,
                enabled: true,
                interval_seconds: 300,
                thresholds: BTreeMap::new(),
            },
            CheckInfo {
                key: CheckKey::parse("mem.used")?,
                enabled: true,
                interval_seconds: 60,
                thresholds: BTreeMap::new(),
            },
        ]
        .to_vec(),
    );

    let (bus, _) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
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
    let server = agent_server(dir.path(), clock.clone(), store.clone(), manager.clone())?;
    let profile_deps = ProfileDeps {
        link: link.clone(),
        agents: Arc::new(DevAgents {
            server: server.clone(),
            link: link.clone(),
        }),
        hosts: store.clone(),
        notes: store.clone(),
        settings: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
    };
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
    let update = UpdateChecker::load(store.clone(), clock.clone()).await;
    let deps = ApiDeps {
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
        actions_service,
        triage: None,
        agents: Arc::new(DevAgents {
            server: server.clone(),
            link: link.clone(),
        }),
        skill_policy: Arc::new(
            SkillPolicy::load(store.clone(), &HostRepo::list_hosts(&*store).await?).await?,
        ),
        profiles: Some(profile_deps.clone()),
        clock: clock.clone(),
        bus,
        config: Arc::new(config),
        notifier: None,
        metrics: None,
        settings: None,
        audit: store.clone(),
        logs: Some(Arc::new(CannedLogSource::new(dev_logs()))),
        redactor: Arc::new(PatternRedactor::default()),
        update,
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
    let found = HostRepo::get_host(&*store, nas).await?;
    let Some(seeded) = found else {
        return Err("the nas host is missing".into());
    };
    domovoi_hub::profile::refresh(&profile_deps, &seeded).await?;

    let listener = tokio::net::TcpListener::bind(ADDR).await?;
    let agents = tokio::net::TcpListener::bind(AGENT_ADDR).await?;
    let serving = server.clone();
    tokio::spawn(async move { serving.serve(agents).await });
    say(format_args!(
        "Agents enroll against wss://{AGENT_ADDR}; hub fingerprint {}",
        server.fingerprint_text()
    ));
    if let Some(link) = auth.open_setup().await? {
        say(format_args!("Open {link}"));
    }
    say(format_args!(
        "Serving on http://{ADDR}. Press Enter to fire an incident on pi; Ctrl+C to stop."
    ));
    tokio::spawn(fire_on_enter(manager, pi, nas));
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.cancel();
        })
        .await?;
    Ok(())
}
