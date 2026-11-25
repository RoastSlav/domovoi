//! The harness of the namespace tests: a real `nft`, a scratch state directory, and the gate that
//! skips every test unless `DOMOVOI_NS_TESTS=1` (set by the job and by `scripts` that run the
//! suite inside `unshare`).

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use async_trait::async_trait;
use domovoi_core::attest::{BootId, ExpectedSafety, SafetyIdentity, UnverifiedReason, ZERO_HASH};
use domovoi_core::containment::templates::{MaxElements, TableName};
use domovoi_core::containment::{ExclusionInputs, Gate, Layout, Net, effective_exclusions};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::identity::{PathFacts, PathKind};
use domovoi_core::ports::{ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};
use domovoi_helper::nft::exec::Runner;
use domovoi_helper::nft::journal_file::Journal;
use domovoi_helper::nft::{Cx, Facts, State, ensure};
use domovoi_helper::state::LockGuard;
use domovoi_testkit::{FakeClock, FakeHostInspect};
use jiff::Timestamp;

/// Whether the namespace tests run. Without it each test prints why it did nothing and passes,
/// so a missing prerequisite is visible and never a failure on a laptop.
pub fn enabled(test: &str) -> bool {
    if std::env::var("DOMOVOI_NS_TESTS").as_deref() == Ok("1") {
        return true;
    }
    eprintln!("SKIPPED {test}: set DOMOVOI_NS_TESTS=1 and run inside a network namespace");
    false
}

/// Runs a real process; the only spawner in these tests.
pub struct RealExec;

#[async_trait]
impl ProcessExecutor for RealExec {
    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
        tokio::task::spawn_blocking(move || {
            let mut command = Command::new(&spec.program);
            command
                .args(&spec.args)
                .env_clear()
                .envs(&spec.env)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = command
                .spawn()
                .map_err(|e| ProcessError::Spawn(e.to_string()))?;
            if let Some(text) = &spec.stdin
                && let Some(mut stdin) = child.stdin.take()
            {
                let _ = stdin.write_all(text.as_bytes());
            }
            let out = child
                .wait_with_output()
                .map_err(|e| ProcessError::Spawn(e.to_string()))?;
            Ok(ProcessOutput {
                exit_code: out.status.code(),
                signal: None,
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                truncated: false,
                timed_out: false,
                duration_ms: 0,
            })
        })
        .await
        .map_err(|e| ProcessError::Spawn(e.to_string()))?
    }
}

fn root_file() -> PathFacts {
    PathFacts {
        kind: Some(PathKind::File),
        symlink: false,
        mode: 0o755,
        uid: 0,
        gid: 0,
    }
}

/// The tests share one network namespace and one table name, so they take turns.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A scratch directory, a host that vets `nft` and `ip`, and a clock.
pub struct Ns {
    _serial: std::sync::MutexGuard<'static, ()>,
    pub dir: tempfile::TempDir,
    pub exec: RealExec,
    pub inspect: FakeHostInspect,
    pub clock: FakeClock,
}

impl Ns {
    pub fn new() -> Self {
        let serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut flush = Command::new("/usr/sbin/nft")
            .args(["-f", "-"])
            .stdin(Stdio::piped())
            .spawn()
            .expect("nft starts");
        if let Some(mut stdin) = flush.stdin.take() {
            let _ = stdin.write_all(b"flush ruleset\n");
        }
        let _ = flush.wait();
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("state.lock"), "").expect("lock");
        let inspect = FakeHostInspect::new()
            .path("/usr/sbin/nft", root_file())
            .path("/usr/sbin/ip", root_file())
            .path("/usr/sbin/conntrack", root_file());
        Self {
            _serial: serial,
            dir,
            exec: RealExec,
            inspect,
            clock: FakeClock::new("2026-01-01T00:00:00Z".parse::<Timestamp>().expect("time")),
        }
    }

    pub fn cx<'a>(
        &'a self,
        guard: &'a LockGuard<'a>,
        max_elements: u32,
        max_active: u32,
    ) -> Cx<'a> {
        Cx {
            run: Runner {
                exec: &self.exec,
                inspect: &self.inspect,
                timeout: jiff::SignedDuration::from_secs(10),
                netns: None,
            },
            clock: &self.clock,
            table: TableName::parse("domovoi").expect("table"),
            max_elements: MaxElements::new(max_elements).expect("max"),
            max_ttl_s: 3600,
            max_active,
            mutations_per_hour: 1000,
            dir: self.dir.path(),
            sys_root: std::path::Path::new("/nonexistent-sys-root"),
            journal: Journal::new(self.dir.path()),
            guard,
        }
    }
}

pub fn exclusions() -> Vec<Net> {
    let inputs = ExclusionInputs {
        management_cidrs: vec![Net::parse("192.0.2.0/24").expect("net")],
        ..ExclusionInputs::default()
    };
    effective_exclusions(Some(&inputs)).expect("exclusions")
}

pub fn facts(gate: Gate) -> Facts {
    let identity = SafetyIdentity {
        fingerprint: KeyFingerprint::of_public_key(&[7; 32]),
        boot_id: BootId::parse("00000000-0000-4000-8000-000000000000").expect("boot id"),
        generation: 1,
        config_hash: ZERO_HASH,
        route_state_hash: ZERO_HASH,
    };
    Facts {
        gate,
        exclusions: Ok(exclusions()),
        identity: Ok(identity),
        key_text: String::new(),
        drills: String::new(),
    }
}

pub const OPEN: Gate = Gate::Open {
    interval_path: true,
};

pub fn unverified() -> ExpectedSafety {
    ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyNotPinned,
    }
}

pub async fn state(cx: &Cx<'_>, gate: Gate) -> State {
    let facts = facts(gate);
    let loaded = cx.journal.load().expect("journal");
    let wanted = match gate {
        Gate::Open {
            interval_path: true,
        } => Some(Layout::HostsAndPrefixes),
        Gate::Open {
            interval_path: false,
        } => Some(Layout::Hosts),
        _ => None,
    };
    let excl = facts.exclusions.clone().ok();
    let ens = ensure::ensure(cx, wanted, excl.as_deref(), &loaded)
        .await
        .expect("ensure");
    State { ens, loaded, facts }
}

/// Sends `script` straight to `nft`, as the harness (not the helper) does.
pub async fn raw(ns: &Ns, script: &str) -> (Option<i32>, String) {
    let spec = ProcessSpec {
        program: "/usr/sbin/nft".to_owned(),
        args: vec!["-f".to_owned(), "-".to_owned()],
        env: BTreeMap::from([("LC_ALL".to_owned(), "C".to_owned())]),
        cwd: None,
        stdin: Some(script.to_owned()),
        timeout: jiff::SignedDuration::from_secs(10),
        max_output_bytes: 65_536,
        run_as: None,
    };
    let out = ns.exec.run(spec).await.expect("nft runs");
    (out.exit_code, out.stderr)
}

/// Polls `done` every 100 ms for at most 15 s of real time: the kernel's timers run on the
/// kernel's clock, so these tests wait for it and never for the fake clock.
pub async fn eventually(mut done: impl AsyncFnMut() -> bool) -> bool {
    for _ in 0..150 {
        if done().await {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}
