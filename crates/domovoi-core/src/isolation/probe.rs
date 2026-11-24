//! The eight-point isolation report: plain data the runner collects in probe mode.
//!
//! The collection runs inside the runner's namespace (`domovoi_helper::probe::collect`); the
//! decision lives in [`super::evaluate_isolation`], which needs no I/O.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::identity::HostDirs;
use crate::ports::{Access, Probe};

/// The result of one probe of one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessRecord {
    /// The path probed (the directory, for the write probes).
    pub path: PathBuf,
    /// What was attempted.
    pub how: Probe,
    /// What happened.
    pub access: Access,
}

impl AccessRecord {
    /// A record of `how` on `path` giving `access`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, how: Probe, access: Access) -> Self {
        Self {
            path: path.into(),
            how,
            access,
        }
    }
}

/// `/run/systemd/incoming`, when it exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingFacts {
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Its owner uid.
    pub uid: u32,
    /// Its permission bits.
    pub mode: u32,
}

/// What the runner sees under `/run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFacts {
    /// Names in `/run`.
    pub run_entries: Vec<String>,
    /// Names in `/run/systemd`; `None` when it does not exist.
    pub systemd_entries: Option<Vec<String>>,
    /// `/run/systemd/incoming`, when it exists.
    pub incoming: Option<IncomingFacts>,
}

/// What a mountinfo entry is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountRole {
    /// The bind of the skills directory.
    SkillsBind,
    /// The bind of the packages directory.
    PackagesBind,
    /// One of the three masks.
    Mask,
}

/// One mount the runner's view depends on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountFact {
    /// Where it is mounted.
    pub point: PathBuf,
    /// What it is for.
    pub role: MountRole,
    /// Whether the `ro` option is set.
    pub read_only: bool,
}

/// The runner's own identity and privileges, from `/proc/self/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcStatus {
    /// Real uid.
    pub uid: u32,
    /// Real gid.
    pub gid: u32,
    /// Supplementary groups.
    pub groups: Vec<u32>,
    /// Effective capability mask.
    pub cap_eff: u64,
    /// Bounding capability mask.
    pub cap_bnd: u64,
    /// The `NoNewPrivs` flag.
    pub no_new_privs: bool,
}

/// Everything the eight points of the isolation check need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationReport {
    /// The directories the probe was aimed at.
    pub dirs: HostDirs,
    /// Point 1: `connect()` to the helper and runner sockets.
    pub connects: Vec<AccessRecord>,
    /// Point 2: `stat`, `list` and `open` of the agent state, helper state and the key.
    pub hidden_state: Vec<AccessRecord>,
    /// Point 3: names in `/etc/domovoi`.
    pub etc_listing: Option<Vec<String>>,
    /// Point 3: names in `/var/lib`.
    pub var_lib_listing: Option<Vec<String>>,
    /// Point 3: opens of `helper.toml` and `agent.toml`.
    pub config_files: Vec<AccessRecord>,
    /// Point 3: what is under `/run`.
    pub run: Option<RunFacts>,
    /// Point 4: create, rename, unlink and append attempts in the two read-only directories.
    pub writes: Vec<AccessRecord>,
    /// Point 5: `/home` and `/root`.
    pub homes: Vec<AccessRecord>,
    /// Point 5: whether `/run/user` exists.
    pub run_user_exists: bool,
    /// Point 6: `/proc/<helper pid>/` and `/proc/1/environ`.
    pub proc_hidden: Vec<AccessRecord>,
    /// Point 7: identity and privileges.
    pub status: ProcStatus,
    /// Point 7: the binds and masks from `/proc/self/mountinfo`.
    pub mounts: Vec<MountFact>,
    /// Point 7: the AppArmor label, informational.
    pub apparmor_label: Option<String>,
    /// Point 8: opening `/etc/shadow`, informational.
    pub shadow: Option<Access>,
}
