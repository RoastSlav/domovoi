//! A helper directory on the real disk and a scripted host around it.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use domovoi_core::attest::{BootId, Nonce32, Report, parse_report};
use domovoi_core::helper::SignedAttestation;
use domovoi_core::identity::{AGENT_ACCOUNT, HostDirs, PathFacts, PathKind, UserEntry};
use domovoi_helper::attest::lifecycle::{KeyInfo, init_attestation};
use domovoi_helper::attest::{AttestEnv, AttestOutcome, safety_attest};
use domovoi_helper::state::StateLayout;
use domovoi_testkit::{FakeClock, FakeHostInspect, FakeProcessExecutor};
use ed25519_dalek::{Signature, VerifyingKey};
use jiff::Timestamp;
use nix::unistd::{Gid, Uid};

use crate::support::{IP, Script};

pub const AGENT_ID: u32 = 900;
pub const BOOT_A: &str = "5f0c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b";
pub const BOOT_B: &str = "6a1d2b3c-4d5e-4f60-9bac-1d2e3f4a5b6c";

pub fn layout() -> StateLayout {
    StateLayout {
        root_uid: Uid::effective().as_raw(),
        root_gid: Gid::effective().as_raw(),
    }
}

pub fn boot(text: &str) -> BootId {
    BootId::parse(text).expect("boot id")
}

pub fn nonce(n: u8) -> Nonce32 {
    Nonce32::from_bytes([n; 32])
}

/// What the scripted host holds that is not on the real disk.
#[derive(Clone)]
pub struct HostText {
    pub helper_toml: String,
    pub agent_toml: String,
    pub boot_id: String,
    /// The mode the scripted directory shows (the real one is always 0700).
    pub dir_mode: u32,
    /// When set, the scripted key file shows this mode.
    pub key_mode: Option<u32>,
}

impl Default for HostText {
    fn default() -> Self {
        Self {
            helper_toml: "[safety]\nmanagement_cidrs = [\"198.51.100.0/24\"]\n".to_owned(),
            agent_toml: "[agent]\nhost_name = \"a\"\nhub_url = \"wss://192.0.2.77:7421\"\n"
                .to_owned(),
            boot_id: format!("{BOOT_A}\n"),
            dir_mode: 0o700,
            key_mode: None,
        }
    }
}

pub struct World {
    _root: tempfile::TempDir,
    pub dirs: HostDirs,
    pub exec: Arc<FakeProcessExecutor>,
    pub clock: Arc<FakeClock>,
    pub text: HostText,
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
    /// A world with a stock routing script and no key yet.
    pub fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("domovoi-attest-")
            .tempdir_in("/tmp")
            .expect("tempdir under /tmp");
        let dirs = HostDirs {
            helper_dir: root.path().join("state"),
            etc_dir: root.path().join("etc"),
            ..HostDirs::default()
        };
        let exec = Arc::new(FakeProcessExecutor::new());
        Script::stock().apply(&exec);
        Self {
            _root: root,
            dirs,
            exec,
            clock: Arc::new(FakeClock::new(
                "2026-07-01T12:00:00Z".parse::<Timestamp>().expect("time"),
            )),
            text: HostText::default(),
        }
    }

    /// Another world over the same helper directory, with its own host script and clock, for a
    /// test that changes what the host looks like between two attestations.
    pub fn sharing(&self) -> World {
        let mut other = World::new();
        other.dirs = self.dirs.clone();
        other.text = self.text.clone();
        other
    }

    pub fn dir(&self) -> &Path {
        &self.dirs.helper_dir
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir().join(name)
    }

    /// Runs the whole of `helper init` (the directory, the lock, the key and the first state).
    pub fn init(&self) -> KeyInfo {
        init_attestation(layout(), self.dir(), &boot(BOOT_A)).expect("init")
    }

    /// The scripted host: the agent account, root-owned facts for each file, the `ip` program and
    /// the texts the helper reads.
    pub fn inspect(&self) -> Arc<FakeHostInspect> {
        let dirs = &self.dirs;
        let agent = UserEntry {
            name: AGENT_ACCOUNT.to_owned(),
            uid: AGENT_ID,
            gid: AGENT_ID,
            shell: "/usr/sbin/nologin".to_owned(),
        };
        let mut dir_facts = root_file(self.text.dir_mode);
        dir_facts.kind = Some(PathKind::Dir);
        let mut host = FakeHostInspect::new()
            .ids(0, 0)
            .user(agent, &[AGENT_ID])
            .path(&dirs.helper_dir, dir_facts)
            .path(dirs.helper_dir.join("state.lock"), root_file(0o600))
            .path(dirs.etc_dir.join("helper.toml"), root_file(0o600))
            .path(IP, root_file(0o755))
            .text(dirs.etc_dir.join("helper.toml"), &self.text.helper_toml)
            .text(dirs.etc_dir.join("agent.toml"), &self.text.agent_toml)
            .text(domovoi_helper::attest::BOOT_ID_PATH, &self.text.boot_id);
        for name in ["attest.key", "state"] {
            if self.path(name).exists() {
                host = host.path(dirs.helper_dir.join(name), root_file(0o600));
            }
        }
        if let Some(mode) = self.text.key_mode {
            host = host.path(dirs.helper_dir.join("attest.key"), root_file(mode));
        }
        Arc::new(host)
    }

    pub fn env(&self) -> AttestEnv {
        AttestEnv::new(
            self.dirs.clone(),
            self.inspect(),
            self.clock.clone(),
            self.exec.clone(),
        )
    }

    pub async fn attest(&self, n: u8) -> AttestOutcome {
        safety_attest(&self.env(), nonce(n)).await
    }

    /// The signed attestation of a challenge, or a panic naming the unsigned token.
    pub async fn signed(&self, n: u8) -> SignedAttestation {
        match self.attest(n).await {
            AttestOutcome::Signed(attestation) => attestation,
            AttestOutcome::Unsigned(token) => panic!("unsigned: {token}"),
        }
    }

    pub fn mode(&self, name: &str) -> u32 {
        mode_of(&self.path(name))
    }

    pub fn clock_now(&self) -> Timestamp {
        domovoi_core::ports::Clock::now(&*self.clock)
    }

    pub fn clock_now_plus(&self, seconds: i64) -> Timestamp {
        self.clock_now() + jiff::SignedDuration::from_secs(seconds)
    }
}

/// The permission bits of `path`.
pub fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o7777
}

/// Verifies `attestation` the way the hub does and parses its report.
pub fn verify(attestation: &SignedAttestation) -> Report {
    let key = VerifyingKey::from_bytes(&attestation.public_key).expect("a valid key");
    let signature = Signature::from_bytes(&attestation.signature);
    key.verify_strict(
        &domovoi_core::attest::signing_bytes(&attestation.report),
        &signature,
    )
    .expect("the signature verifies over the signing bytes");
    parse_report(&attestation.report).expect("the report parses strictly")
}

/// The generation of a complete report.
pub fn generation(attestation: &SignedAttestation) -> u64 {
    match verify(attestation) {
        Report::Complete(report) => report.generation,
        Report::Incomplete(report) => panic!("incomplete: {}", report.reason.as_str()),
    }
}

/// The reason of a signed incomplete report.
pub fn reason(attestation: &SignedAttestation) -> String {
    match verify(attestation) {
        Report::Incomplete(report) => report.reason.as_str().to_owned(),
        Report::Complete(_) => panic!("expected an incomplete report"),
    }
}
