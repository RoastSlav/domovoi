//! The harness: a scratch state directory, a scripted host and a fake `nft`, and the pieces of
//! `State` an operation reads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use domovoi_core::attest::{BootId, ExpectedSafety, SafetyIdentity, UnverifiedReason, ZERO_HASH};
use domovoi_core::containment::templates::{MaxElements, TableName};
use domovoi_core::containment::{ExclusionInputs, Gate, Layout, Net, effective_exclusions};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::identity::{PathFacts, PathKind};
use domovoi_core::ports::Clock;
use domovoi_helper::nft::exec::Runner;
use domovoi_helper::nft::journal_file::Journal;
use domovoi_helper::nft::{Cx, Facts, State, ensure};
use domovoi_helper::state::{LockGuard, StateLock};
use domovoi_testkit::{FakeClock, FakeHostInspect, ScriptedProcessExecutor};
use jiff::Timestamp;

use super::fake::{Nft, Shared, respond};

pub type Hook = Arc<dyn Fn(&domovoi_core::ports::ProcessSpec) + Send + Sync>;

pub const MANAGEMENT: &str = "192.0.2.0/24";

pub struct World {
    pub dir: tempfile::TempDir,
    pub clock: Arc<FakeClock>,
    pub nft: Shared,
    pub exec: ScriptedProcessExecutor,
    pub inspect: FakeHostInspect,
    pub table: String,
    pub max_active: u32,
    pub max_ttl_s: u64,
    pub mutations_per_hour: u32,
}

fn root_file(mode: u32) -> PathFacts {
    PathFacts {
        kind: Some(PathKind::File),
        symlink: false,
        mode,
        uid: 0,
        gid: 0,
    }
}

impl World {
    pub fn new() -> Self {
        Self::with_hook(None)
    }

    /// A world whose fake `nft` calls `hook` with each spec before answering it.
    pub fn with_hook(hook: Option<Hook>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("state.lock"), "").expect("lock file");
        let clock = Arc::new(FakeClock::new(
            "2026-01-01T00:00:00Z"
                .parse::<Timestamp>()
                .expect("timestamp"),
        ));
        let nft: Shared = Arc::new(Mutex::new(Nft {
            conntrack_ok: true,
            ..Nft::default()
        }));
        let inner = respond(nft.clone(), clock.clone());
        let exec = ScriptedProcessExecutor::new(move |spec| {
            if let Some(hook) = &hook {
                hook(spec);
            }
            inner(spec)
        });
        let inspect = FakeHostInspect::new()
            .path("/usr/sbin/nft", root_file(0o755))
            .path("/usr/sbin/conntrack", root_file(0o755));
        Self {
            dir,
            clock,
            nft,
            exec,
            inspect,
            table: "domovoi".to_owned(),
            max_active: 10,
            max_ttl_s: 3600,
            mutations_per_hour: 100,
        }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn lock(&self) -> StateLock {
        StateLock::open(self.path()).expect("lock")
    }

    pub fn cx<'a>(&'a self, guard: &'a LockGuard<'a>, sys_root: &'a Path) -> Cx<'a> {
        Cx {
            run: Runner {
                exec: &self.exec,
                inspect: &self.inspect,
                timeout: jiff::SignedDuration::from_secs(5),
                netns: None,
            },
            clock: self.clock.as_ref(),
            table: TableName::parse(&self.table).expect("table"),
            max_elements: MaxElements::new(1000).expect("max"),
            max_ttl_s: self.max_ttl_s,
            max_active: self.max_active,
            mutations_per_hour: self.mutations_per_hour,
            dir: self.path(),
            sys_root,
            journal: Journal::new(self.path()),
            guard,
        }
    }

    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub fn advance(&self, secs: i64) {
        self.clock.advance(jiff::SignedDuration::from_secs(secs));
    }

    pub fn scripts(&self) -> Vec<String> {
        self.nft.lock().expect("nft").scripts.clone()
    }
}

pub fn identity(generation: u64) -> SafetyIdentity {
    SafetyIdentity {
        fingerprint: KeyFingerprint::of_public_key(&[7; 32]),
        boot_id: BootId::parse("00000000-0000-4000-8000-000000000000").expect("boot id"),
        generation,
        config_hash: ZERO_HASH,
        route_state_hash: ZERO_HASH,
    }
}

pub fn verified(generation: u64) -> ExpectedSafety {
    ExpectedSafety::from(&identity(generation))
}

pub fn unverified() -> ExpectedSafety {
    ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyNotPinned,
    }
}

pub fn exclusions() -> Vec<Net> {
    let inputs = ExclusionInputs {
        management_cidrs: vec![Net::parse(MANAGEMENT).expect("net")],
        ..ExclusionInputs::default()
    };
    effective_exclusions(Some(&inputs)).expect("exclusions")
}

pub fn facts(gate: Gate) -> Facts {
    Facts {
        gate,
        exclusions: Ok(exclusions()),
        identity: Ok(identity(1)),
        key_text: String::new(),
        drills: String::new(),
    }
}

pub const OPEN: Gate = Gate::Open {
    interval_path: true,
};
pub const OPEN_HOSTS: Gate = Gate::Open {
    interval_path: false,
};

/// Ensures the table and builds the state an operation reads, with `facts`.
pub async fn state(cx: &Cx<'_>, facts: Facts) -> State {
    let loaded = cx.journal.load().expect("journal");
    let wanted = match facts.gate {
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

pub fn no_sys() -> PathBuf {
    PathBuf::from("/nonexistent-sys-root")
}
