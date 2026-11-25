//! ST-07 and ST-09: what systemd says about the shipped units, and what their text contains.
//!
//! Every query goes through the injected [`ProcessExecutor`]. A failing `systemctl show` or a
//! unit with no fragment is "not inspectable", never a skip and never a pass; a drop-in or a
//! pending reload is "modified" before any `daemon-reload` is needed to see it.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::identity::{AGENT_ACCOUNT, AGENT_GROUP, CheckToken, HostDirs, SKILL_ACCOUNT};
use domovoi_core::isolation::{CheckId, CheckResult, Outcome};
use domovoi_core::ports::{HostInspect, ProcessExecutor, ProcessSpec};
use jiff::SignedDuration;

use super::duration::{SystemdDuration, parse_systemd_duration};

/// The instance name the self-test asks about. The bare template name is rejected by every
/// measured systemd, so a unit of the template is always queried through this instance.
pub const SELFTEST_INSTANCE: &str = "selftest";

/// The helper's socket unit.
pub const HELPER_SOCKET_UNIT: &str = "domovoi-helper.socket";
/// The helper's per-connection service template, without the instance and suffix.
const HELPER_SERVICE_TEMPLATE: &str = "domovoi-helper@";
/// The skill runner's socket unit.
pub const RUNNER_SOCKET_UNIT: &str = "domovoi-skill-runner.socket";
/// The skill runner's per-connection service template, without the instance and suffix.
const RUNNER_SERVICE_TEMPLATE: &str = "domovoi-skill-runner@";
/// The agent's unit.
pub const AGENT_UNIT: &str = "domovoi-agent.service";

/// Properties asked of every unit.
const COMMON_PROPERTIES: &[&str] = &[
    "LoadState",
    "FragmentPath",
    "DropInPaths",
    "NeedDaemonReload",
];

/// What one property must be.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Expect {
    /// The exact text; a property systemd does not print counts as empty.
    Text(String),
    /// A file mode, compared as a number so `0660` and `660` agree.
    Mode(u32),
    /// A duration, compared after parsing so `1min` and `60s` agree.
    Span(SystemdDuration),
    /// The `ExecStart` argument vector after the program path, as `systemctl show` prints it in
    /// `argv[]=`; a difference is reported as `bind_paths` (the unit and `agent.toml` disagree).
    Argv(Vec<String>),
    /// The `BindReadOnlyPaths` entries, in order; a difference is reported as `bind_paths`.
    Binds(Vec<String>),
}

/// A unit to inspect and the properties it must have.
struct UnitSpec {
    name: String,
    /// A unit that may be absent: a host without it is not a failure.
    optional: bool,
    expect: Vec<(&'static str, Expect)>,
}

fn text(value: &str) -> Expect {
    Expect::Text(value.to_owned())
}

/// The runner's `ExecStart` arguments, as the unit is rendered for `dirs`.
fn runner_argv(dirs: &HostDirs) -> Vec<String> {
    [
        "skill-runner",
        "serve",
        "--skills-dir",
        &dirs.skills_dir.to_string_lossy(),
        "--packages-dir",
        &dirs.packages_dir.to_string_lossy(),
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect()
}

fn specs(dirs: &HostDirs) -> Vec<UnitSpec> {
    vec![
        UnitSpec {
            optional: false,
            name: HELPER_SOCKET_UNIT.to_owned(),
            expect: vec![
                ("Accept", text("yes")),
                ("SocketMode", Expect::Mode(0o660)),
                ("SocketUser", text("root")),
                ("SocketGroup", text(AGENT_GROUP)),
            ],
        },
        UnitSpec {
            optional: false,
            name: format!("{HELPER_SERVICE_TEMPLATE}{SELFTEST_INSTANCE}.service"),
            expect: vec![
                ("Type", text("simple")),
                ("User", text("")),
                (
                    "RuntimeMaxUSec",
                    Expect::Span(SystemdDuration::from_secs(60)),
                ),
            ],
        },
        UnitSpec {
            optional: false,
            name: AGENT_UNIT.to_owned(),
            expect: vec![("User", text(AGENT_ACCOUNT)), ("Group", text(AGENT_GROUP))],
        },
        UnitSpec {
            optional: true,
            name: RUNNER_SOCKET_UNIT.to_owned(),
            expect: vec![
                ("Accept", text("yes")),
                ("SocketMode", Expect::Mode(0o660)),
                ("SocketUser", text("root")),
                ("SocketGroup", text(AGENT_GROUP)),
            ],
        },
        UnitSpec {
            optional: true,
            name: format!("{RUNNER_SERVICE_TEMPLATE}{SELFTEST_INSTANCE}.service"),
            expect: vec![
                ("Type", text("simple")),
                ("User", text(SKILL_ACCOUNT)),
                (
                    "RuntimeMaxUSec",
                    Expect::Span(SystemdDuration::from_secs(3720)),
                ),
                ("ExecStart", Expect::Argv(runner_argv(dirs))),
                (
                    "BindReadOnlyPaths",
                    Expect::Binds(vec![
                        dirs.skills_dir.to_string_lossy().into_owned(),
                        dirs.packages_dir.to_string_lossy().into_owned(),
                    ]),
                ),
            ],
        },
    ]
}

/// Splits `systemctl show` output into properties. Lines without `=` are ignored.
#[must_use]
pub fn parse_show(output: &str) -> BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

fn parse_mode(value: &str) -> Option<u32> {
    u32::from_str_radix(value.trim(), 8).ok()
}

/// What was found out about one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInspection {
    /// The unit name as queried.
    pub unit: String,
    /// Its fragment path when systemd named one.
    pub fragment: Option<String>,
    /// Why the unit cannot be inspected; non-empty means ST-07 fails with `unit_not_inspectable`.
    pub not_inspectable: Vec<String>,
    /// How the unit differs from the shipped one; non-empty means `unit_modified`.
    pub modified: Vec<String>,
}

/// The arguments after the program path in the `argv[]=` part of a `systemctl show` `ExecStart`.
fn exec_argv(value: &str) -> Option<Vec<String>> {
    let (_, rest) = value.split_once("argv[]=")?;
    let argv = rest.split(" ; ").next()?;
    Some(argv.split_whitespace().skip(1).map(str::to_owned).collect())
}

fn compare(found: &BTreeMap<String, String>, name: &str, expect: &Expect) -> Option<String> {
    let value = found.get(name).map_or("", String::as_str);
    match expect {
        Expect::Text(want) if value == want => None,
        Expect::Text(want) => Some(format!("{name}={value:?}, expected {want:?}")),
        Expect::Mode(want) => match parse_mode(value) {
            Some(mode) if mode == *want => None,
            _ => Some(format!("{name}={value:?}, expected {want:04o}")),
        },
        Expect::Span(want) => match parse_systemd_duration(value) {
            Some(span) if span == *want => None,
            _ => Some(format!("{name}={value:?}, expected {want:?}")),
        },
        Expect::Argv(want) => match exec_argv(value) {
            Some(argv) if argv == *want => None,
            other => Some(format!(
                "bind_paths: ExecStart arguments are {other:?}, expected {want:?} for agent.toml"
            )),
        },
        Expect::Binds(want) => {
            let have: Vec<&str> = value.split_whitespace().collect();
            if have == *want {
                None
            } else {
                Some(format!(
                    "bind_paths: {name}={value:?}, expected {want:?} for agent.toml"
                ))
            }
        }
    }
}

fn judge(spec: &UnitSpec, props: &BTreeMap<String, String>) -> UnitInspection {
    let value = |key: &str| props.get(key).map_or("", String::as_str);
    let fragment = Some(value("FragmentPath").to_owned()).filter(|f| !f.is_empty());
    let mut out = UnitInspection {
        unit: spec.name.clone(),
        fragment,
        not_inspectable: Vec::new(),
        modified: Vec::new(),
    };
    if out.fragment.is_none() {
        out.not_inspectable.push("FragmentPath is empty".to_owned());
        return out;
    }
    if value("LoadState") != "loaded" {
        out.modified.push(format!(
            "LoadState={:?}, expected \"loaded\"",
            value("LoadState")
        ));
    }
    if !value("DropInPaths").is_empty() {
        out.modified
            .push(format!("drop-ins present: {}", value("DropInPaths")));
    }
    if value("NeedDaemonReload") == "yes" {
        out.modified
            .push("NeedDaemonReload=yes: the unit changed on disk".to_owned());
    }
    for (name, expect) in &spec.expect {
        out.modified.extend(compare(props, name, expect));
    }
    out
}

/// Asks systemd about the helper socket, a helper service instance and the agent unit.
///
/// `systemctl` is the absolute path of the program. The runner's socket and service instance are
/// compared too when installed (the service against the rendering for `dirs`); a host without
/// them is not a failure.
pub async fn inspect_units(
    executor: &dyn ProcessExecutor,
    systemctl: &str,
    dirs: &HostDirs,
) -> Vec<UnitInspection> {
    let mut out = Vec::new();
    for spec in specs(dirs) {
        let mut names: Vec<&str> = COMMON_PROPERTIES.to_vec();
        names.extend(spec.expect.iter().map(|(n, _)| *n));
        let run = ProcessSpec {
            program: systemctl.to_owned(),
            args: vec![
                "show".to_owned(),
                "--no-pager".to_owned(),
                format!("--property={}", names.join(",")),
                spec.name.clone(),
            ],
            env: BTreeMap::new(),
            cwd: None,
            stdin: None,
            timeout: SignedDuration::from_secs(10),
            max_output_bytes: 65_536,
            run_as: None,
        };
        let inspection = match executor.run(run).await {
            Ok(o) if o.exit_code == Some(0) && !o.timed_out => judge(&spec, &parse_show(&o.stdout)),
            Ok(o) => failed(&spec, format!("systemctl show exited {:?}", o.exit_code)),
            Err(e) => failed(&spec, format!("systemctl show could not run: {e}")),
        };
        if spec.optional && inspection.fragment.is_none() {
            continue;
        }
        out.push(inspection);
    }
    out
}

fn failed(spec: &UnitSpec, why: String) -> UnitInspection {
    UnitInspection {
        unit: spec.name.clone(),
        fragment: None,
        not_inspectable: vec![why],
        modified: Vec::new(),
    }
}

/// ST-07 from the inspections. A unit that cannot be inspected fails the check before a
/// modification is considered; runner units are `nd` and not part of this result.
#[must_use]
pub fn st07(inspections: &[UnitInspection]) -> CheckResult {
    let describe = |pick: fn(&UnitInspection) -> &Vec<String>| -> Vec<String> {
        inspections
            .iter()
            .flat_map(|i| pick(i).iter().map(move |p| format!("{}: {p}", i.unit)))
            .collect()
    };
    let not_inspectable = describe(|i| &i.not_inspectable);
    let modified = describe(|i| &i.modified);
    let outcome = if !not_inspectable.is_empty() {
        Outcome::fail(CheckToken::UnitNotInspectable, not_inspectable.join("; "))
    } else if !modified.is_empty() {
        Outcome::fail(CheckToken::UnitModified, modified.join("; "))
    } else {
        Outcome::Pass
    };
    CheckResult::new(CheckId::St07, outcome)
}

/// Something in a unit's text that ST-09 forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitTextFault {
    /// A numeric uid or gid where a name belongs.
    HardcodedId {
        /// The directive.
        key: String,
        /// Its value.
        value: String,
    },
    /// An executable under `/run`, `/tmp` or `/dev/shm`, which may be mounted `noexec`.
    ExecUnderRun {
        /// The directive.
        key: String,
        /// The program path.
        path: String,
    },
}

const ID_KEYS: &[&str] = &[
    "User",
    "Group",
    "SocketUser",
    "SocketGroup",
    "SupplementaryGroups",
];
const EXEC_KEYS: &[&str] = &["ExecStart", "ExecStartPre", "ExecStopPost"];

/// Lines of a unit with comments dropped and `\` continuations joined.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if pending.is_empty() && (line.starts_with('#') || line.starts_with(';')) {
            continue;
        }
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
            continue;
        }
        pending.push_str(line);
        out.push(std::mem::take(&mut pending));
    }
    if !pending.is_empty() {
        out.push(pending);
    }
    out
}

fn under_run(path: &str) -> bool {
    ["/run", "/tmp", "/dev/shm"]
        .iter()
        .any(|root| path == *root || path.strip_prefix(root).is_some_and(|r| r.starts_with('/')))
}

/// ST-09 over the text of one unit file.
#[must_use]
pub fn scan_unit_text(text: &str) -> Vec<UnitTextFault> {
    let mut faults = Vec::new();
    for line in logical_lines(text) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if ID_KEYS.contains(&key) {
            for item in value.split_whitespace() {
                if item.bytes().all(|b| b.is_ascii_digit()) {
                    faults.push(UnitTextFault::HardcodedId {
                        key: key.to_owned(),
                        value: item.to_owned(),
                    });
                }
            }
        } else if EXEC_KEYS.contains(&key) {
            let program = value
                .trim_start_matches(['@', '-', ':', '+', '!'])
                .split_whitespace()
                .next()
                .unwrap_or("");
            if under_run(program) {
                faults.push(UnitTextFault::ExecUnderRun {
                    key: key.to_owned(),
                    path: program.to_owned(),
                });
            }
        }
    }
    faults
}

/// ST-09 over the fragments ST-07 found. A fragment that cannot be read fails the check; with no
/// fragment at all the check is `nd`.
#[must_use]
pub fn st09(inspect: &dyn HostInspect, inspections: &[UnitInspection]) -> CheckResult {
    let fragments: Vec<(&str, &str)> = inspections
        .iter()
        .filter_map(|i| Some((i.unit.as_str(), i.fragment.as_deref()?)))
        .collect();
    if fragments.is_empty() {
        return CheckResult::new(CheckId::St09, Outcome::nd("no unit file was located"));
    }
    let mut ids = Vec::new();
    let mut execs = Vec::new();
    let mut unreadable = Vec::new();
    for (unit, fragment) in fragments {
        match inspect.read_text(Path::new(fragment)) {
            Err(e) => unreadable.push(format!("{unit}: cannot read {fragment}: {e}")),
            Ok(body) => {
                for fault in scan_unit_text(&body) {
                    match fault {
                        UnitTextFault::HardcodedId { key, value } => {
                            ids.push(format!("{unit}: {key}={value}"));
                        }
                        UnitTextFault::ExecUnderRun { key, path } => {
                            execs.push(format!("{unit}: {key} runs {path}"));
                        }
                    }
                }
            }
        }
    }
    let outcome = if !unreadable.is_empty() {
        Outcome::fail(CheckToken::UnitNotInspectable, unreadable.join("; "))
    } else if !ids.is_empty() {
        Outcome::fail(CheckToken::UnitHardcodedId, ids.join("; "))
    } else if !execs.is_empty() {
        Outcome::fail(CheckToken::UnitExecUnderRun, execs.join("; "))
    } else {
        Outcome::Pass
    };
    CheckResult::new(CheckId::St09, outcome)
}
