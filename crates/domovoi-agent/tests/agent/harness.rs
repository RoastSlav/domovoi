//! Shared pieces: identities, fake time, scripted checks and a running link.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_agent::LinkError;
use domovoi_agent::identity::{Identity, generate_key};
use domovoi_agent::link::tls::PinnedVerifier;
use domovoi_agent::link::ws::TlsConnector;
use domovoi_agent::link::{IncomingRequest, IncomingTransfer, Link, LinkHandle, LinkParts};
use domovoi_agent::spool::{SharedSpool, Spool};
use domovoi_checks::framework::{Check, CheckContext, CheckError, CheckReport, Observation};
use domovoi_core::{AgentId, CheckKey, HostId, Severity, SignalClass, TargetKey};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, FixtureHostFs};
use jiff::SignedDuration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::hub::TestHub;

pub fn identity(hub: &TestHub) -> Identity {
    Identity::new(
        AgentId::new(),
        HostId::new(),
        generate_key(),
        hub.fingerprint,
    )
}

pub fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(T0))
}

pub fn empty_fs(dir: &tempfile::TempDir) -> Arc<FixtureHostFs> {
    let root = dir.path().join("host");
    let proc = root.join("proc");
    std::fs::create_dir_all(proc.join("sys/kernel/random")).unwrap();
    std::fs::write(
        proc.join("sys/kernel/random/boot_id"),
        "018f0000-0000-7000-8000-000000000001\n",
    )
    .unwrap();
    std::fs::write(proc.join("uptime"), "1.00 1.00\n").unwrap();
    Arc::new(FixtureHostFs::new(root))
}

/// Yields until the tasks that can make progress have made it.
pub async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

/// A link running against the test hub.
pub struct RunningLink {
    pub task: JoinHandle<Result<(), LinkError>>,
    pub handle: LinkHandle,
    pub requests: mpsc::Receiver<IncomingRequest>,
    pub shutdown: CancellationToken,
}

pub fn start_link(
    hub: &TestHub,
    identity: &Identity,
    clock: &Arc<FakeClock>,
    dir: &tempfile::TempDir,
) -> RunningLink {
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    start_link_with(hub, identity, clock, dir, spool)
}

pub fn start_link_with(
    hub: &TestHub,
    identity: &Identity,
    clock: &Arc<FakeClock>,
    dir: &tempfile::TempDir,
    spool: Arc<SharedSpool>,
) -> RunningLink {
    start_link_full(
        hub,
        identity,
        clock,
        dir,
        spool,
        domovoi_agent::link::handshake::AgentStatusSource::default(),
        None,
    )
}

/// A link whose runtime status a test can change between probes.
pub fn start_link_with_status(
    hub: &TestHub,
    identity: &Identity,
    clock: &Arc<FakeClock>,
    dir: &tempfile::TempDir,
    status: domovoi_agent::link::handshake::AgentStatusSource,
) -> RunningLink {
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    start_link_full(hub, identity, clock, dir, spool, status, None)
}

/// A link that forwards `transfer_*` frames to `transfers`.
pub fn start_link_transfers(
    hub: &TestHub,
    identity: &Identity,
    clock: &Arc<FakeClock>,
    dir: &tempfile::TempDir,
    transfers: mpsc::Sender<IncomingTransfer>,
) -> RunningLink {
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    let status = domovoi_agent::link::handshake::AgentStatusSource::default();
    start_link_full(hub, identity, clock, dir, spool, status, Some(transfers))
}

fn start_link_full(
    hub: &TestHub,
    identity: &Identity,
    clock: &Arc<FakeClock>,
    dir: &tempfile::TempDir,
    spool: Arc<SharedSpool>,
    status: domovoi_agent::link::handshake::AgentStatusSource,
    transfers: Option<mpsc::Sender<IncomingTransfer>>,
) -> RunningLink {
    let connector = Arc::new(TlsConnector::new(
        hub.url.clone(),
        Arc::new(PinnedVerifier::new(identity.hub_fingerprint)),
        clock.clone(),
    ));
    let parts = LinkParts {
        connector,
        identity: identity.clone(),
        clock: clock.clone(),
        fs: empty_fs(dir),
        skills: Vec::new(),
        checks: Vec::new(),
        docker: None,
        extensions: domovoi_agent::link::handshake::HelloExtensions {
            journal_epoch: uuid::Uuid::now_v7(),
            ..Default::default()
        },
        status,
        overrides: None,
        spool,
        jitter: Box::new(|| 1.0),
        status_dir: Some(dir.path().to_path_buf()),
        transfers,
    };
    let (link, handle, requests) = Link::new(parts);
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(link.run(shutdown.clone()));
    RunningLink {
        task,
        handle,
        requests,
        shutdown,
    }
}

pub fn key(name: &str) -> CheckKey {
    CheckKey::parse(name).unwrap()
}

/// Makes every spool push fail: a directory where the spool file belongs, so it cannot be
/// opened for append or renamed over.
pub fn block_spool(dir: &std::path::Path) {
    let path = dir.join(domovoi_agent::spool::SPOOL_FILE);
    if path.is_file() {
        std::fs::remove_file(&path).unwrap();
    }
    std::fs::create_dir(&path).unwrap();
}

/// Undoes [`block_spool`].
pub fn unblock_spool(dir: &std::path::Path) {
    std::fs::remove_dir(dir.join(domovoi_agent::spool::SPOOL_FILE)).unwrap();
}

pub fn target(name: &str) -> Option<TargetKey> {
    Some(TargetKey::parse(name).unwrap())
}

pub fn firing(name: &str, severity: Severity) -> Observation {
    Observation::firing(target(name), severity, format!("{name} is down"))
}

pub fn ok(name: &str) -> Observation {
    Observation::ok(target(name), format!("{name} is fine"))
}

/// A check that returns scripted reports, one per run, repeating the last.
pub struct ScriptedCheck {
    key: CheckKey,
    reports: Mutex<VecDeque<CheckReport>>,
    pub runs: AtomicUsize,
}

impl ScriptedCheck {
    pub fn new(name: &str, reports: Vec<CheckReport>) -> Self {
        Self {
            key: key(name),
            reports: Mutex::new(reports.into()),
            runs: AtomicUsize::new(0),
        }
    }
}

pub fn report(observations: Vec<Observation>) -> CheckReport {
    CheckReport {
        observations,
        facts: Vec::new(),
    }
}

#[async_trait]
impl Check for ScriptedCheck {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_secs(60)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let n = self.runs.fetch_add(1, Ordering::SeqCst) + 1;
        ctx.state.set("runs", n.to_string());
        let mut reports = self.reports.lock().unwrap();
        let report = if reports.len() > 1 {
            reports.pop_front()
        } else {
            reports.front().cloned()
        };
        Ok(report.unwrap_or_default())
    }
}
