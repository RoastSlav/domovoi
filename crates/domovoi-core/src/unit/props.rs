//! What the manager and the disk report about a unit, as plain data the pure code can hash.

use std::collections::BTreeSet;

use super::name::UnitId;

/// The unit file states a plain, supported unit may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitFileState {
    /// Enabled by a symlink under `/etc`.
    Enabled,
    /// Present and not enabled.
    Disabled,
    /// Has no install section.
    Static,
    /// Enabled through another unit.
    Indirect,
}

impl UnitFileState {
    /// The systemd spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Static => "static",
            Self::Indirect => "indirect",
        }
    }
}

/// What `LoadUnit` and `GetAll` on the Unit interface say about one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitProps {
    /// The canonical `Id`.
    pub id: String,
    /// Every name the unit answers to.
    pub names: BTreeSet<String>,
    /// `LoadState`, such as `loaded`, `not-found` or `masked`.
    pub load_state: String,
    /// `UnitFileState` as reported; empty when none.
    pub unit_file_state: String,
    /// Whether the unit is transient.
    pub transient: bool,
    /// `FragmentPath`.
    pub fragment_path: String,
    /// `SourcePath`.
    pub source_path: String,
    /// `DropInPaths`, in the order the manager applies them.
    pub drop_in_paths: Vec<String>,
    /// `NeedDaemonReload`.
    pub need_daemon_reload: bool,
    /// `TriggeredBy`.
    pub triggered_by: BTreeSet<String>,
    /// `Triggers`.
    pub triggers: BTreeSet<String>,
}

/// One `Exec*` command: the program, its argument vector and whether failure is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecCommand {
    /// The program path.
    pub path: String,
    /// The argument vector.
    pub argv: Vec<String>,
    /// Whether a failure is ignored (`-` prefix).
    pub ignore_failure: bool,
}

/// The service-interface properties of a loaded unit that a restart would run. Runtime fields
/// (`start_time`, `pid`, exit code and status) are already removed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedProps {
    /// `ExecStart`.
    pub exec_start: Vec<ExecCommand>,
    /// `ExecStartPre`.
    pub exec_start_pre: Vec<ExecCommand>,
    /// `ExecStartPost`.
    pub exec_start_post: Vec<ExecCommand>,
    /// `ExecStop`.
    pub exec_stop: Vec<ExecCommand>,
    /// `ExecReload`, argument vectors only.
    pub exec_reload: Vec<Vec<String>>,
    /// `Environment`, in the order given.
    pub environment: Vec<String>,
    /// `EnvironmentFiles` paths.
    pub environment_files: BTreeSet<String>,
    /// `User`.
    pub user: String,
    /// `Group`.
    pub group: String,
    /// `WorkingDirectory`.
    pub working_directory: String,
    /// `Restart`.
    pub restart: String,
    /// `Type`.
    pub service_type: String,
    /// `StartLimitIntervalUSec`.
    pub start_limit_interval_usec: u64,
    /// `StartLimitBurst`.
    pub start_limit_burst: u32,
    /// `Requires`.
    pub requires: BTreeSet<String>,
    /// `BindsTo`.
    pub binds_to: BTreeSet<String>,
    /// `PartOf`.
    pub part_of: BTreeSet<String>,
    /// `Wants`.
    pub wants: BTreeSet<String>,
}

/// The runtime facts the restart reads before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitRuntime {
    /// `InvocationID`; empty when the unit never ran.
    pub invocation_id: String,
    /// `ExecMainPID`; zero when none.
    pub main_pid: u32,
    /// `ActiveState`.
    pub active_state: String,
    /// The service `Result`, such as `success` or `start-limit-hit`.
    pub result: String,
    /// `NRestarts`.
    pub n_restarts: u32,
}

/// What a started process runs, read from `/proc/<pid>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessView {
    /// `environ` entries as `NAME=value`.
    pub environ: Vec<String>,
    /// `cmdline` arguments.
    pub cmdline: Vec<String>,
    /// The real user id.
    pub uid: u32,
}

/// The directories the helper may restart units from.
pub const ALLOWED_DIRS: [&str; 2] = ["/etc/systemd/system/", "/usr/lib/systemd/system/"];

/// The helper's rules for which units may be bound.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitPolicy {
    /// Exact canonical ids the helper may restart.
    pub allow: BTreeSet<UnitId>,
    /// Ids and names never restarted.
    pub protected: BTreeSet<String>,
}
