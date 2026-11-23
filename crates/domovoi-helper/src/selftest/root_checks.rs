//! The checks root makes: ST-03, ST-04, ST-07 to ST-11 and ST-17 to ST-20.
//!
//! Everything reads through the [`HostInspect`] port and the injected [`ProcessExecutor`]; no
//! check changes the host, and a path that is off its mode is reported, never repaired.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::identity::{
    AGENT_ACCOUNT, AGENT_GROUP, CheckToken, DOCKER_GROUP, FactsError, FaultKind, HostDirs,
    JOURNAL_GROUP, PathFacts, PathFault, PathKind, PathSpec, SKILL_ACCOUNT, TreeRule,
    evaluate_docker_membership, evaluate_path, evaluate_skill_account, evaluate_tree,
    expected_paths,
};
use domovoi_core::isolation::{CheckId, CheckResult, Outcome, parse_proc_status};
use domovoi_core::ports::{HostInspect, InspectError, ProcessExecutor, ProcessSpec};
use jiff::SignedDuration;

use super::units::{inspect_units, st07, st09};

/// ST-20 warns when the median write cycle takes longer than this many microseconds.
pub const SLOW_WRITE_MICROS: u64 = 100_000;
/// ST-20 measures this many write cycles.
pub const WRITE_CYCLES: u32 = 10;
/// CAP_SYS_ADMIN, the capability a privileged container holds and an ordinary one does not.
const CAP_SYS_ADMIN: u64 = 1 << 21;

/// What the root checks found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootChecks {
    /// One result per root check, in id order.
    pub results: Vec<CheckResult>,
    /// The first line of `systemctl --version`; `None` means the host has no systemd.
    pub systemd_version: Option<String>,
}

/// Every departure from the path table, split by the check that reports it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableFaults {
    /// ST-10: the agent, packages, skills and configuration paths.
    pub table: Vec<PathFault>,
    /// ST-11: the helper's directory, lock, key, state and platform record.
    pub helper_state: Vec<PathFault>,
}

impl TableFaults {
    /// Every fault, ST-10's first.
    #[must_use]
    pub fn all(&self) -> Vec<PathFault> {
        self.table
            .iter()
            .chain(&self.helper_state)
            .cloned()
            .collect()
    }
}

fn facts_of(inspect: &dyn HostInspect, spec: &PathSpec) -> Result<PathFacts, FactsError> {
    match spec.kind {
        PathKind::File => inspect.open_facts(&spec.path),
        PathKind::Dir | PathKind::Socket => inspect.lstat(&spec.path),
    }
    .map_err(Into::into)
}

fn tree_faults(inspect: &dyn HostInspect, spec: &PathSpec, agent: (u32, u32)) -> Vec<PathFault> {
    match inspect.walk_tree(&spec.path) {
        Ok(entries) => evaluate_tree(spec, &entries),
        Err(InspectError::NotFound) => Vec::new(),
        Err(_) => vec![PathFault {
            id: spec.id,
            path: spec.path.clone(),
            found: None,
            expected: spec.expected_facts(agent),
            why: FaultKind::Unreadable,
        }],
    }
}

/// Compares every row of the path table except the socket with the host.
///
/// Files are inspected through a descriptor opened without following links, directories without
/// following a symlink. Returns `None` when the `domovoi-agent` account cannot be resolved, since
/// ownership cannot then be decided.
#[must_use]
pub fn table_faults(inspect: &dyn HostInspect, dirs: &HostDirs) -> Option<TableFaults> {
    let agent = inspect.user(AGENT_ACCOUNT).ok().flatten()?;
    let ids = (agent.uid, agent.gid);
    let mut out = TableFaults::default();
    for spec in expected_paths(dirs) {
        if spec.id == domovoi_core::identity::PathId::HelperSocket {
            continue;
        }
        let mut faults: Vec<PathFault> = evaluate_path(&spec, facts_of(inspect, &spec), ids)
            .into_iter()
            .collect();
        if spec.tree != TreeRule::None && spec.kind == PathKind::Dir {
            faults.extend(tree_faults(inspect, &spec, ids));
        }
        if spec.id.is_helper_state() {
            out.helper_state.extend(faults);
        } else {
            out.table.extend(faults);
        }
    }
    Some(out)
}

fn path_check(id: CheckId, token: CheckToken, faults: Vec<PathFault>) -> CheckResult {
    if faults.is_empty() {
        return CheckResult::new(id, Outcome::Pass);
    }
    let detail = format!("{} path(s) off the table", faults.len());
    CheckResult::new(id, Outcome::fail(token, detail)).with_faults(faults)
}

fn st03(inspect: &dyn HostInspect) -> CheckResult {
    let look = || -> Result<Outcome, InspectError> {
        let Some(agent) = inspect.user(AGENT_ACCOUNT)? else {
            return Ok(Outcome::fail(
                CheckToken::AgentAccountMissing,
                "the domovoi-agent account does not exist",
            ));
        };
        let Some(skill) = inspect.user(SKILL_ACCOUNT)? else {
            return Ok(Outcome::fail(
                CheckToken::SkillAccountInvalid,
                "the domovoi-skill account does not exist",
            ));
        };
        let groups_of = inspect.groups_of(&skill)?;
        let mut listed = false;
        for name in [JOURNAL_GROUP, DOCKER_GROUP, AGENT_GROUP] {
            if let Some(group) = inspect.group(name)? {
                listed |= group.members.contains(&skill.name);
            }
        }
        Ok(
            match evaluate_skill_account(&skill, agent.uid, &groups_of, listed) {
                Ok(()) => Outcome::Pass,
                Err(fault) => Outcome::fail(CheckToken::SkillAccountInvalid, fault.to_string()),
            },
        )
    };
    let outcome =
        look().unwrap_or_else(|e| Outcome::nd(format!("accounts could not be read: {e}")));
    CheckResult::new(CheckId::St03, outcome)
}

fn st04(inspect: &dyn HostInspect) -> CheckResult {
    let outcome = match inspect.group(DOCKER_GROUP) {
        Err(e) => Outcome::nd(format!("the docker group could not be read: {e}")),
        Ok(group) => match evaluate_docker_membership(group.as_ref()) {
            Ok(()) => Outcome::Pass,
            Err(names) => Outcome::fail(
                CheckToken::DockerGroupMember,
                format!("in the docker group: {}", names.join(", ")),
            ),
        },
    };
    CheckResult::new(CheckId::St04, outcome)
}

/// The first line of `systemctl --version` when systemd is PID 1 here; `None` for a host without it.
pub async fn detect_systemd(
    inspect: &dyn HostInspect,
    executor: &dyn ProcessExecutor,
    systemctl: &str,
) -> Option<String> {
    let running = inspect
        .lstat(Path::new("/run/systemd/system"))
        .is_ok_and(|f| f.kind == Some(PathKind::Dir));
    if !running {
        return None;
    }
    let spec = ProcessSpec {
        program: systemctl.to_owned(),
        args: vec!["--version".to_owned()],
        env: BTreeMap::new(),
        cwd: None,
        stdin: None,
        timeout: SignedDuration::from_secs(10),
        max_output_bytes: 8192,
        run_as: None,
    };
    let output = executor.run(spec).await.ok()?;
    if output.exit_code != Some(0) {
        return None;
    }
    let line = output.stdout.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

fn no_systemd(id: CheckId) -> CheckResult {
    CheckResult::new(id, Outcome::nd("no_systemd"))
}

fn is_container(inspect: &dyn HostInspect) -> bool {
    ["/.dockerenv", "/run/.containerenv"]
        .iter()
        .any(|p| inspect.lstat(Path::new(p)).is_ok())
}

/// ST-17 over the text of `/proc/self/mountinfo` and `/proc/self/status`.
fn container_surface(mountinfo: &str, status: &str, helper_dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (Some(point), Some(options)) = (fields.get(4), fields.get(5)) else {
            continue;
        };
        let read_only = options.split(',').any(|o| o == "ro");
        if point.ends_with("docker.sock") && !read_only {
            found.push(format!("{point} is mounted read-write"));
        }
        if Path::new(point).starts_with(helper_dir) {
            found.push(format!("{point} binds the helper directory"));
        }
    }
    if parse_proc_status(status).is_ok_and(|s| s.cap_eff & CAP_SYS_ADMIN != 0) {
        found.push("the container holds CAP_SYS_ADMIN (privileged)".to_owned());
    }
    found
}

fn st17(inspect: &dyn HostInspect, dirs: &HostDirs) -> CheckResult {
    if !is_container(inspect) {
        return CheckResult::new(CheckId::St17, Outcome::nd("not_a_container_install"));
    }
    let read = |p: &str| inspect.read_text(Path::new(p));
    let outcome = match (read("/proc/self/mountinfo"), read("/proc/self/status")) {
        (Ok(mounts), Ok(status)) => {
            let found = container_surface(&mounts, &status, &dirs.helper_dir);
            if found.is_empty() {
                Outcome::Pass
            } else {
                Outcome::fail(CheckToken::ContainerEscapeSurface, found.join("; "))
            }
        }
        (Err(e), _) | (_, Err(e)) => Outcome::nd(format!("container facts unreadable: {e}")),
    };
    CheckResult::new(CheckId::St17, outcome)
}

fn st18(inspect: &dyn HostInspect, dirs: &HostDirs) -> CheckResult {
    let outcome = match inspect.lstat(&dirs.etc_dir) {
        Err(InspectError::NotFound) => Outcome::Pass,
        Err(e) => Outcome::nd(format!(
            "{} could not be inspected: {e}",
            dirs.etc_dir.display()
        )),
        Ok(facts) => {
            let empty = inspect
                .list_dir(&dirs.etc_dir)
                .map(|names| names.is_empty());
            match empty {
                Ok(true)
                    if facts.kind == Some(PathKind::Dir)
                        && facts.mode == 0o755
                        && facts.uid == 0 =>
                {
                    Outcome::fail(
                        CheckToken::HostDirectoryCreatedByMask,
                        format!(
                            "{} is an empty root-owned 0755 directory, as a mount namespace mask leaves it",
                            dirs.etc_dir.display()
                        ),
                    )
                }
                Ok(_) => Outcome::Pass,
                Err(e) => Outcome::nd(format!(
                    "{} could not be listed: {e}",
                    dirs.etc_dir.display()
                )),
            }
        }
    };
    CheckResult::new(CheckId::St18, outcome)
}

fn mount_options(mountinfo: &str, point: &str) -> String {
    mountinfo
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.get(4) == Some(&point) {
                f.get(5).map(|o| (*o).to_owned())
            } else {
                None
            }
        })
        .next_back()
        .unwrap_or_else(|| "not a mount point".to_owned())
}

fn st19(inspect: &dyn HostInspect) -> CheckResult {
    let text = |path: &str| {
        inspect
            .read_text(Path::new(path))
            .map_or_else(|_| "absent".to_owned(), |t| t.trim().to_owned())
    };
    let mounts = inspect
        .read_text(Path::new("/proc/self/mountinfo"))
        .unwrap_or_default();
    let detail = format!(
        "lsm={}; yama_ptrace_scope={}; apparmor_restrict_unprivileged_userns={}; apparmor_restrict_unprivileged_userns_complain={}; /run options={}; /tmp options={}",
        text("/sys/kernel/security/lsm"),
        text("/proc/sys/kernel/yama/ptrace_scope"),
        text("/proc/sys/kernel/apparmor_restrict_unprivileged_userns"),
        text("/proc/sys/kernel/apparmor_restrict_unprivileged_userns_complain"),
        mount_options(&mounts, "/run"),
        mount_options(&mounts, "/tmp"),
    );
    CheckResult::new(CheckId::St19, Outcome::info(detail))
}

fn st20(inspect: &dyn HostInspect, dirs: &HostDirs) -> CheckResult {
    let outcome = match inspect.state_write_timings(&dirs.helper_dir, WRITE_CYCLES) {
        Err(e) => Outcome::nd(format!("write timings unavailable: {e}")),
        Ok(micros) if micros.is_empty() => Outcome::nd("no write timings were taken"),
        Ok(mut micros) => {
            micros.sort_unstable();
            let median = micros[micros.len() / 2];
            if median > SLOW_WRITE_MICROS {
                Outcome::warn(
                    CheckToken::StateWriteSlow,
                    format!("median state write {median} microseconds, above {SLOW_WRITE_MICROS}"),
                )
            } else {
                Outcome::Pass
            }
        }
    };
    CheckResult::new(CheckId::St20, outcome)
}

/// Runs the root checks in id order and finds the systemd version.
///
/// `systemctl` is the absolute path of the program, which is run only through `executor`;
/// `version` is what [`detect_systemd`] found.
pub async fn run_root_checks(
    inspect: &dyn HostInspect,
    executor: &dyn ProcessExecutor,
    dirs: &HostDirs,
    systemctl: &str,
    version: Option<String>,
) -> RootChecks {
    let (st07_result, st08_result, st09_result) = if let Some(v) = &version {
        let units = inspect_units(executor, systemctl).await;
        (
            st07(&units),
            CheckResult::new(CheckId::St08, Outcome::info(v.clone())),
            st09(inspect, &units),
        )
    } else {
        (
            no_systemd(CheckId::St07),
            no_systemd(CheckId::St08),
            no_systemd(CheckId::St09),
        )
    };
    let (st10_result, st11_result) = match table_faults(inspect, dirs) {
        Some(faults) => (
            path_check(CheckId::St10, CheckToken::PathModeWrong, faults.table),
            path_check(
                CheckId::St11,
                CheckToken::HelperStatePermissions,
                faults.helper_state,
            ),
        ),
        None => {
            let missing = || {
                Outcome::fail(
                    CheckToken::AgentAccountMissing,
                    "the domovoi-agent account does not exist, so ownership cannot be checked",
                )
            };
            (
                CheckResult::new(CheckId::St10, missing()),
                CheckResult::new(CheckId::St11, missing()),
            )
        }
    };
    RootChecks {
        results: vec![
            st03(inspect),
            st04(inspect),
            st07_result,
            st08_result,
            st09_result,
            st10_result,
            st11_result,
            st17(inspect, dirs),
            st18(inspect, dirs),
            st19(inspect),
            st20(inspect, dirs),
        ],
        systemd_version: version,
    }
}
