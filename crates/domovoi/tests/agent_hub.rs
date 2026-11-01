//! The real agent runtime against the real hub agent server, over TLS on 127.0.0.1.

#[cfg(test)]
mod common {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use domovoi_agent::spool::Spool;
    use domovoi_agent::{AgentDeps, AgentRuntime, EnrollEnv, enroll};
    use domovoi_checks::system::{FsStats, FsUsage};
    use domovoi_config::{AgentConfig, MapEnv};
    use domovoi_core::ports::{AgentLink, FsError, Presence};
    use domovoi_core::{CheckKey, HostId, HostName, Severity, Signal, SourceKind};
    use domovoi_hub::agent_server::{AgentServer, AgentServerDeps, NetProbes, ServerSettings};
    use domovoi_proto::EnrollToken;
    use domovoi_testkit::{
        FakeClock, FakeProcessExecutor, FixtureHostFs, InMemoryStore, ListRedactor,
        RecordingSignalSink,
    };
    use jiff::{SignedDuration, Timestamp};
    use tempfile::TempDir;
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;
    use url::Url;

    pub const GUARD: Duration = Duration::from_secs(10);

    pub fn t0() -> Timestamp {
        "2026-10-31T12:00:00Z".parse().unwrap()
    }

    struct Up;

    #[async_trait]
    impl NetProbes for Up {
        async fn gateway_reachable(&self) -> bool {
            true
        }

        async fn tcp_reachable(&self, _target: &str) -> bool {
            true
        }
    }

    struct NoStats;

    #[async_trait]
    impl FsStats for NoStats {
        async fn usage(&self, _mount_point: &str) -> Result<FsUsage, FsError> {
            Err(FsError::NotFound)
        }
    }

    pub struct Hub {
        pub server: AgentServer,
        pub sink: Arc<RecordingSignalSink>,
        pub url: Url,
        _dir: TempDir,
    }

    pub async fn hub() -> Hub {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!(
            "wss://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        ))
        .unwrap();
        let settings = ServerSettings {
            hub_name: "testhub".to_owned(),
            data_dir: dir.path().to_path_buf(),
            bind: listener.local_addr().unwrap(),
            public_url: url.clone(),
            self_check: "1.1.1.1:53".to_owned(),
            heartbeat: SignedDuration::from_secs(10),
            probes: BTreeMap::new(),
        };
        let store = Arc::new(InMemoryStore::new());
        let sink = Arc::new(RecordingSignalSink::new());
        let deps = AgentServerDeps {
            clock: Arc::new(FakeClock::new(t0())),
            hosts: store.clone(),
            facts: store,
            sink: sink.clone(),
            probes: Arc::new(Up),
            hub_host: HostId::new(),
        };
        let server = AgentServer::new(settings, deps).unwrap();
        let serving = server.clone();
        tokio::spawn(async move { serving.serve(listener).await });
        Hub {
            server,
            sink,
            url,
            _dir: dir,
        }
    }

    pub struct Agent {
        pub host: HostId,
        pub dir: TempDir,
        pub shutdown: CancellationToken,
    }

    impl Drop for Agent {
        fn drop(&mut self) {
            self.shutdown.cancel();
        }
    }

    fn config(dir: &Path, hub: &Url) -> AgentConfig {
        let text = format!(
            "[agent]\nhost_name = \"nas\"\nhub_url = \"{hub}\"\nstate_dir = '{}'\nskills_dir = '{}'\n",
            dir.display(),
            dir.join("skills").display(),
        );
        domovoi_config::parse(&text, &MapEnv::new()).unwrap().0
    }

    pub fn signal(host: HostId) -> Signal {
        Signal::firing(
            SourceKind::Agent,
            host,
            CheckKey::parse("disk.free").unwrap(),
            None,
            Severity::Warning,
            "/srv has 4% free",
            t0(),
        )
    }

    /// Enrolls a real agent with a token, spools one signal and starts its runtime.
    pub async fn agent(hub: &Hub, prepare: impl FnOnce(&Path)) -> Agent {
        let dir = tempfile::tempdir().unwrap();
        let invite = hub
            .server
            .create_token(&HostName::parse("nas").unwrap())
            .await
            .unwrap();
        let clock = Arc::new(FakeClock::new(t0()));
        let fs = Arc::new(FixtureHostFs::new(dir.path().join("host")));
        let config = config(dir.path(), &hub.url);
        let identity = tokio::time::timeout(
            GUARD,
            enroll(
                &hub.url,
                EnrollToken::new(invite.token.expose()),
                Some(hub.server.fingerprint()),
                &config,
                dir.path(),
                EnrollEnv {
                    clock: &*clock,
                    fs: &*fs,
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(identity.host_id, invite.host);
        assert_eq!(identity.hub_fingerprint, hub.server.fingerprint());
        Spool::open(dir.path())
            .unwrap()
            .push(vec![signal(invite.host)])
            .unwrap();
        prepare(dir.path());
        let deps = AgentDeps {
            clock,
            executor: Arc::new(FakeProcessExecutor::new()),
            hostfs: fs.clone(),
            toolfs: fs,
            fs_stats: Arc::new(NoStats),
            container_source: None,
            redactor: Arc::new(ListRedactor::new(Vec::<String>::new())),
        };
        let runtime = AgentRuntime::new(config, deps).await.unwrap();
        let shutdown = CancellationToken::new();
        tokio::spawn(runtime.run(shutdown.clone()));
        Agent {
            host: invite.host,
            dir,
            shutdown,
        }
    }

    /// Yields until the hub sees the agent connected.
    pub async fn connected(hub: &Hub, host: HostId) {
        let link = hub.server.link();
        tokio::time::timeout(GUARD, async {
            while !matches!(link.presence(host), Presence::Connected { .. }) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the agent never connected");
    }

    /// Yields until the hub's sink holds `n` signals.
    pub async fn signals(hub: &Hub, n: usize) -> Vec<Signal> {
        tokio::time::timeout(GUARD, async {
            loop {
                let got: Vec<Signal> = hub.sink.batches().into_iter().flatten().collect();
                if got.len() >= n {
                    return got;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the signals never arrived")
    }
}

use std::collections::BTreeMap;

use common::{agent, connected, hub, signals, t0};
use domovoi_core::ports::{AgentLink, LinkError, SkillDispatch};
use domovoi_core::{ActionId, HostId, PlanHash, ReadToolCall, Risk, SkillName};
use jiff::SignedDuration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_agent_enrolls_connects_and_sends_signals() {
    let hub = hub().await;
    let agent = agent(&hub, |_| {}).await;
    connected(&hub, agent.host).await;
    let got = signals(&hub, 1).await;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].host(), agent.host);
    assert_eq!(got[0].check().as_str(), "disk.free");
    assert_eq!(got[0].summary(), "/srv has 4% free");
    assert!(agent.dir.path().join("identity.json").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_tool_round_trips_and_times_out() {
    let hub = hub().await;
    let agent = agent(&hub, |_| {}).await;
    connected(&hub, agent.host).await;
    let link = hub.server.link();
    let deadline = t0() + SignedDuration::from_secs(30);
    let out = link
        .run_tool(agent.host, ReadToolCall::HostOverview, deadline)
        .await;
    assert!(
        !matches!(
            out,
            Err(LinkError::NotConnected | LinkError::Timeout | LinkError::Transport(_))
        ),
        "the agent answered: {out:?}"
    );
    let late = link
        .run_tool(agent.host, ReadToolCall::HostOverview, t0())
        .await;
    assert_eq!(
        late,
        Err(LinkError::Timeout),
        "a passed deadline fails at once"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_skill_reaches_agent_and_plan_mismatch_is_reported() {
    let hub = hub().await;
    let agent = agent(&hub, |dir| install(&dir.join("skills"), "restart_app")).await;
    connected(&hub, agent.host).await;
    let skill = SkillName::parse("restart_app").unwrap();
    let link = hub.server.link();
    // The hub validates against what the agent announced. Skills load on Unix only;
    // elsewhere a bare manifest stands in and the agent reports it has no such skill.
    let (validated, manifest_hash) = match link.skills(agent.host).into_iter().next() {
        Some(summary) => {
            let args = BTreeMap::from([("service".to_owned(), "plex".to_owned())]);
            (summary.validate_args(&args).unwrap(), summary.manifest_hash)
        }
        None => {
            let manifest = domovoi_testkit::build::manifest("restart_app", Risk::Write).unwrap();
            (
                manifest.validate_args(&BTreeMap::new()).unwrap(),
                manifest.manifest_hash(),
            )
        }
    };
    // Approved for a different host, so the agent's own plan hash cannot match.
    let dispatch = SkillDispatch {
        action: ActionId::new(),
        plan_hash: PlanHash::compute(&skill, manifest_hash, HostId::new(), &validated),
        skill,
        args: validated,
    };
    let out = link
        .run_skill(agent.host, dispatch, t0() + SignedDuration::from_secs(30))
        .await;
    let expected = if cfg!(unix) {
        "PlanMismatch"
    } else {
        "NotFound"
    };
    match out {
        Err(LinkError::Rejected(why)) => assert!(why.starts_with(expected), "{why}"),
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[cfg(test)]
fn install(dir: &std::path::Path, name: &str) {
    let folder = dir.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = format!(
        "name = \"{name}\"\ndescription = \"Test skill\"\nrisk = \"write\"\nentry = \"run\"\n\
         timeout = \"10s\"\nmax_output_bytes = 4096\n\n[[params]]\nname = \"service\"\n\
         description = \"A service\"\nkind = {{ type = \"string\", pattern = '^[a-z]{{1,16}}$', max_len = 16 }}\n"
    );
    std::fs::write(folder.join("skill.toml"), manifest).unwrap();
    std::fs::write(folder.join("run"), "#!/bin/sh\necho ran\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(folder.join("run"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
