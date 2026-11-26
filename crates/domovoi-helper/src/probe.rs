//! The eight-point isolation probe: collects what the skill runner can see and reach.
//!
//! It runs inside the runner's namespace in probe mode and only gathers facts through the
//! [`HostInspect`] port; [`domovoi_core::isolation::evaluate_isolation`] decides. A fact that
//! cannot be read is recorded as the worst case, so a broken probe never reads as isolated.

use std::path::{Path, PathBuf};

use domovoi_core::identity::{HostDirs, PathKind};
use domovoi_core::isolation::{
    AccessRecord, IncomingFacts, IsolationReport, MountFact, MountRole, ProcStatus, RunFacts,
    parse_proc_status,
};
use domovoi_core::ports::{HostInspect, Probe};
use domovoi_core::skill_runner::RUNNER_SOCKET_PATH;

/// What the probe needs to know that it cannot see itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeArgs {
    /// The pid of the helper instance, whose `/proc/<pid>` must be hidden (point 6).
    pub helper_pid: u32,
}

fn record(inspect: &dyn HostInspect, path: &Path, how: Probe) -> AccessRecord {
    AccessRecord::new(path, how, inspect.probe(path, how))
}

/// The mount points the runner's view is built from: the three masks hide these directories.
const MASKS: [&str; 3] = ["/etc/domovoi", "/var/lib", "/run"];

fn mounts(mountinfo: &str, dirs: &HostDirs) -> Vec<MountFact> {
    let mut out = Vec::new();
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (Some(point), Some(options)) = (fields.get(4), fields.get(5)) else {
            continue;
        };
        let role = if Path::new(point) == dirs.skills_dir {
            MountRole::SkillsBind
        } else if Path::new(point) == dirs.packages_dir {
            MountRole::PackagesBind
        } else if MASKS.contains(point) {
            MountRole::Mask
        } else {
            continue;
        };
        out.push(MountFact {
            point: PathBuf::from(point),
            role,
            read_only: options.split(',').any(|o| o == "ro"),
        });
    }
    out
}

fn run_facts(inspect: &dyn HostInspect) -> Option<RunFacts> {
    let run_entries = inspect.list_dir(Path::new("/run")).ok()?;
    let systemd_entries = inspect.list_dir(Path::new("/run/systemd")).ok();
    let incoming = inspect
        .lstat(Path::new("/run/systemd/incoming"))
        .ok()
        .map(|f| IncomingFacts {
            is_dir: f.kind == Some(PathKind::Dir),
            uid: f.uid,
            mode: f.mode,
        });
    Some(RunFacts {
        run_entries,
        systemd_entries,
        incoming,
    })
}

/// The first regular file under `dir` in path order, or none when the tree holds no file or
/// cannot be walked.
fn first_file(inspect: &dyn HostInspect, dir: &Path) -> Option<PathBuf> {
    inspect
        .walk_tree(dir)
        .ok()?
        .into_iter()
        .find(|entry| entry.facts.kind == Some(PathKind::File))
        .map(|entry| entry.path)
}

fn write_probes(inspect: &dyn HostInspect, dirs: &[&PathBuf]) -> Vec<AccessRecord> {
    let mut out = Vec::new();
    for dir in dirs {
        for how in [Probe::Create, Probe::Rename, Probe::Unlink] {
            out.push(record(inspect, dir, how));
        }
        // Append to a regular file: a directory, such as a starter skill's own folder, answers
        // EISDIR whatever the mount allows, which says nothing about writability.
        if let Some(file) = first_file(inspect, dir) {
            out.push(record(inspect, &file, Probe::Append));
        }
    }
    out
}

/// A status that fails every check: used when `/proc/self/status` cannot be read.
fn worst_status() -> ProcStatus {
    ProcStatus {
        uid: 0,
        gid: 0,
        groups: Vec::new(),
        cap_eff: u64::MAX,
        cap_bnd: u64::MAX,
        no_new_privs: false,
    }
}

/// Collects the eight points of the isolation check through `inspect`.
#[must_use]
pub fn collect(inspect: &dyn HostInspect, args: &ProbeArgs) -> IsolationReport {
    let dirs = HostDirs::default();
    let key = dirs.helper_dir.join("attest.key");
    let hidden_state = vec![
        record(inspect, &dirs.state_dir, Probe::Stat),
        record(inspect, &dirs.state_dir, Probe::List),
        record(inspect, &dirs.helper_dir, Probe::Stat),
        record(inspect, &dirs.helper_dir, Probe::List),
        record(inspect, &key, Probe::Stat),
        record(inspect, &key, Probe::Open),
    ];
    let proc_pid = PathBuf::from(format!("/proc/{}", args.helper_pid));
    let text = |path: &str| inspect.read_text(Path::new(path));
    let status = text("/proc/self/status")
        .ok()
        .and_then(|t| parse_proc_status(&t).ok())
        .unwrap_or_else(worst_status);
    let mountinfo = text("/proc/self/mountinfo").unwrap_or_default();
    IsolationReport {
        connects: vec![
            record(inspect, &dirs.helper_socket, Probe::Connect),
            record(inspect, Path::new(RUNNER_SOCKET_PATH), Probe::Connect),
        ],
        hidden_state,
        etc_listing: inspect.list_dir(&dirs.etc_dir).ok(),
        var_lib_listing: inspect.list_dir(Path::new("/var/lib")).ok(),
        config_files: vec![
            record(inspect, &dirs.etc_dir.join("helper.toml"), Probe::Open),
            record(inspect, &dirs.etc_dir.join("agent.toml"), Probe::Open),
        ],
        run: run_facts(inspect),
        writes: write_probes(inspect, &[&dirs.skills_dir, &dirs.packages_dir]),
        homes: vec![
            record(inspect, Path::new("/home"), Probe::List),
            record(inspect, Path::new("/root"), Probe::List),
        ],
        run_user_exists: inspect.lstat(Path::new("/run/user")).is_ok(),
        proc_hidden: vec![
            record(inspect, &proc_pid, Probe::Stat),
            record(inspect, Path::new("/proc/1/environ"), Probe::Open),
        ],
        status,
        mounts: mounts(&mountinfo, &dirs),
        apparmor_label: text("/proc/self/attr/current")
            .ok()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty()),
        shadow: Some(inspect.probe(Path::new("/etc/shadow"), Probe::Open)),
        dirs,
    }
}
