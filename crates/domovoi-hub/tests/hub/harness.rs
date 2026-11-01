//! A hub agent server on 127.0.0.1 with fakes for every port, and helpers to drive time.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::{HostId, HostName, Signal};
use domovoi_hub::agent_server::{AgentServer, AgentServerDeps, NetProbes, ServerSettings};
use domovoi_testkit::{FakeClock, InMemoryStore, RecordingSignalSink};
use jiff::{SignedDuration, Timestamp};
use tempfile::TempDir;
use tokio::net::TcpListener;
use url::Url;

/// A real-time guard so a broken test fails instead of hanging. Never a sleep.
pub const GUARD: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn guard<F: Future>(fut: F) -> F::Output {
    tokio::time::timeout(GUARD, fut)
        .await
        .expect("the test waited too long")
}

pub fn t0() -> Timestamp {
    "2026-10-31T12:00:00Z".parse().unwrap()
}

/// Polls `fut`, advancing the clock by `step` whenever it is not ready, so a hub timer
/// that `fut` waits on fires however late it was started.
pub async fn pump<F: Future>(clock: &FakeClock, step: SignedDuration, fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    guard(async {
        loop {
            tokio::select! {
                biased;
                out = &mut fut => return out,
                () = tokio::task::yield_now() => clock.advance(step),
            }
        }
    })
    .await
}

/// Network answers the tests choose.
pub struct FakeProbes {
    pub gateway: AtomicBool,
    pub outside: AtomicBool,
    pub hosts_up: AtomicBool,
    pub self_check: String,
    pub asked: Mutex<Vec<String>>,
}

impl FakeProbes {
    fn new(self_check: &str) -> Self {
        Self {
            gateway: AtomicBool::new(true),
            outside: AtomicBool::new(true),
            hosts_up: AtomicBool::new(true),
            self_check: self_check.to_owned(),
            asked: Mutex::new(Vec::new()),
        }
    }

    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait]
impl NetProbes for FakeProbes {
    async fn gateway_reachable(&self) -> bool {
        self.gateway.load(Ordering::SeqCst)
    }

    async fn tcp_reachable(&self, target: &str) -> bool {
        self.asked.lock().unwrap().push(target.to_owned());
        if target == self.self_check {
            self.outside.load(Ordering::SeqCst)
        } else {
            self.hosts_up.load(Ordering::SeqCst)
        }
    }
}

pub fn settings(dir: &TempDir) -> ServerSettings {
    ServerSettings {
        hub_name: "testhub".to_owned(),
        data_dir: dir.path().to_path_buf(),
        bind: "127.0.0.1:0".parse().unwrap(),
        public_url: Url::parse("wss://testhub:7421").unwrap(),
        self_check: "1.1.1.1:53".to_owned(),
        heartbeat: SignedDuration::from_secs(10),
        probes: BTreeMap::new(),
    }
}

/// A running agent server and the fakes behind it.
pub struct Hub {
    pub server: AgentServer,
    pub clock: Arc<FakeClock>,
    pub store: Arc<InMemoryStore>,
    pub sink: Arc<RecordingSignalSink>,
    pub probes: Arc<FakeProbes>,
    pub addr: SocketAddr,
    pub hub_host: HostId,
    pub dir: TempDir,
}

impl Hub {
    pub async fn start() -> Self {
        Self::start_with(|_| {}).await
    }

    pub async fn start_with(edit: impl FnOnce(&mut ServerSettings)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = settings(&dir);
        edit(&mut settings);
        let clock = Arc::new(FakeClock::new(t0()));
        let store = Arc::new(InMemoryStore::new());
        let sink = Arc::new(RecordingSignalSink::new());
        let probes = Arc::new(FakeProbes::new(&settings.self_check));
        let hub_host = HostId::new();
        let deps = AgentServerDeps {
            clock: clock.clone(),
            hosts: store.clone(),
            facts: store.clone(),
            sink: sink.clone(),
            probes: probes.clone(),
            hub_host,
        };
        let server = AgentServer::new(settings, deps).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serving = server.clone();
        tokio::spawn(async move { serving.serve(listener).await });
        Self {
            server,
            clock,
            store,
            sink,
            probes,
            addr,
            hub_host,
            dir,
        }
    }

    /// Every signal submitted so far, in order.
    pub fn signals(&self) -> Vec<Signal> {
        self.sink.batches().into_iter().flatten().collect()
    }

    pub async fn token(&self, host: &str) -> (HostId, String) {
        let invite = self
            .server
            .create_token(&HostName::parse(host).unwrap())
            .await
            .unwrap();
        (invite.host, invite.token.expose().to_owned())
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.server.shutdown();
    }
}
