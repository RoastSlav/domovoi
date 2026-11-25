//! The skill runner rules: the unit rules of `domovoi-skill-runner@.service` and its socket
//! (UNIT-1, UNIT-2, UNIT-3, UNIT-5 and UNIT-6) and the installer's runner order (INSTALL-1) (ADR
//! 0034). The source scans RUNNER-1 and RUNNER-2 are in `runner_scan.rs`.
//!
//! The unit rules reuse the parser of [`super::units`]. A unit file proves nothing about the
//! view a process gets; the run-time checks of `domovoi agent selftest` read that. These rules
//! only keep the shipped text, and every rendering of it, from regressing.

use super::Violation;
use super::identity_rules::violation;
use super::rules::{INSTALL_SCRIPT, SOCKET_GROUP, UNITS_DIR};
use super::units::{Entry, UnitFile};

/// The runner template unit.
pub const RUNNER_SERVICE: &str = "domovoi-skill-runner@.service";

/// The runner socket unit.
pub const RUNNER_SOCKET: &str = "domovoi-skill-runner.socket";

/// The one path the runner socket listens on (UNIT-5).
pub const RUNNER_SOCKET_PATH: &str = "/run/domovoi-skill-runner.sock";

/// The account the runner unit runs as (UNIT-2).
const RUNNER_ACCOUNT: &str = "domovoi-skill";

/// The mask of the runner unit: three parents, never a child under one of them (UNIT-1).
const RUNNER_MASK: &str = "/etc/domovoi:ro /var/lib:ro /run:ro";

/// The measured baseline of the runner unit, `(key, value)` in `[Service]` (UNIT-3). `ExecStart`
/// and `BindReadOnlyPaths` hold the configured directories and are checked by shape. No
/// `MemoryMax=`, `TasksMax=` or hardening candidate is required: each needs a measurement
/// first. An empty value is a reset and is written out on purpose.
pub const RUNNER_UNIT_DIRECTIVES: &[(&str, &str)] = &[
    ("Type", "simple"),
    ("User", RUNNER_ACCOUNT),
    ("StandardInput", "socket"),
    ("StandardOutput", "journal"),
    ("StandardError", "journal"),
    ("Restart", "no"),
    ("NoNewPrivileges", "yes"),
    ("CapabilityBoundingSet", ""),
    ("AmbientCapabilities", ""),
    ("RestrictSUIDSGID", "yes"),
    ("ProtectSystem", "strict"),
    ("ProtectHome", "yes"),
    ("PrivateTmp", "yes"),
    ("PrivateDevices", "yes"),
    ("ProtectProc", "invisible"),
    ("ProcSubset", "pid"),
    ("TemporaryFileSystem", RUNNER_MASK),
    ("RuntimeMaxSec", "3720s"),
    ("KillMode", "control-group"),
    ("TimeoutStopSec", "5s"),
];

fn at(unit: &UnitFile, line: usize) -> String {
    format!("{UNITS_DIR}/{}:{line}", unit.name)
}

fn unit_at(unit: &UnitFile) -> String {
    format!("{UNITS_DIR}/{}", unit.name)
}

fn entries<'a>(unit: &'a UnitFile, key: &'a str) -> impl Iterator<Item = &'a Entry> {
    unit.entries.iter().filter(move |e| e.key == key)
}

/// A path as a mask or bind names it: no `-`, `+`, `!` or `~` prefix, no trailing `/`.
fn clean(path: &str) -> &str {
    let path = path.trim_start_matches(['-', '+', '!', '~']);
    if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    }
}

/// Whether `path` is `parent` or lies under it.
fn within(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| parent == "/" || rest.starts_with('/'))
}

/// The paths a bind directive value names, source and destination of each entry.
fn bind_paths(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in value.split_whitespace() {
        let token = token.trim_start_matches(['-', '+', '!', '~']);
        for segment in token.split(':').take(2) {
            if segment.starts_with('/') {
                out.push(clean(segment).to_string());
            }
        }
    }
    out
}

/// Run every runner unit rule over `units`: UNIT-1 and UNIT-6 over every unit, the runner
/// clauses of UNIT-2, UNIT-3 and UNIT-5 over the runner's two units when they are present
/// ([`check_runner_present`] says whether they must be).
pub fn check_runner_units(units: &[UnitFile]) -> Vec<Violation> {
    let mut out = Vec::new();
    for unit in units {
        out.extend(check_inaccessible_under_mask(unit));
        out.extend(check_bind_targets(unit));
    }
    out.extend(check_runner_service(units));
    out.extend(check_runner_socket(units));
    out
}

/// The shipped tree holds both runner units (UNIT-3 for the service, UNIT-5 for the socket).
pub fn check_runner_present(units: &[UnitFile]) -> Vec<Violation> {
    let mut out = Vec::new();
    if !units.iter().any(|u| u.name == RUNNER_SERVICE) {
        out.push(violation(
            "UNIT-3",
            &format!("{UNITS_DIR}/{RUNNER_SERVICE}"),
            "the skill runner unit is missing",
        ));
    }
    if !units.iter().any(|u| u.name == RUNNER_SOCKET) {
        out.push(violation(
            "UNIT-5",
            &format!("{UNITS_DIR}/{RUNNER_SOCKET}"),
            "the skill runner socket is missing",
        ));
    }
    out
}

/// UNIT-1: no `InaccessiblePaths=` entry sits at or under a `TemporaryFileSystem=` parent. The
/// child of a masked parent fails the start with `226/NAMESPACE`, and `systemd-analyze verify`
/// stays silent about it.
fn check_inaccessible_under_mask(unit: &UnitFile) -> Vec<Violation> {
    let parents: Vec<String> = entries(unit, "TemporaryFileSystem")
        .flat_map(|e| e.value.split_whitespace())
        .map(|token| clean(token.split(':').next().unwrap_or_default()).to_string())
        .collect();
    let mut out = Vec::new();
    for entry in entries(unit, "InaccessiblePaths") {
        for token in entry.value.split_whitespace() {
            let path = clean(token);
            if let Some(parent) = parents.iter().find(|p| within(path, p)) {
                out.push(violation(
                    "UNIT-1",
                    &at(unit, entry.line),
                    &format!(
                        "`InaccessiblePaths=` names `{path}` under the masked parent `{parent}`"
                    ),
                ));
            }
        }
    }
    out
}

/// UNIT-6: no bind directive names a `.sock` path or anything under `/run`.
fn check_bind_targets(unit: &UnitFile) -> Vec<Violation> {
    let mut out = Vec::new();
    for key in ["BindPaths", "BindReadOnlyPaths"] {
        for entry in entries(unit, key) {
            for path in bind_paths(&entry.value) {
                let reason = if path.ends_with(".sock") {
                    Some("a socket node")
                } else if within(&path, "/run") {
                    Some("a path under /run")
                } else {
                    None
                };
                if let Some(reason) = reason {
                    out.push(violation(
                        "UNIT-6",
                        &at(unit, entry.line),
                        &format!("`{key}=` binds `{path}`, {reason}"),
                    ));
                }
            }
        }
    }
    out
}

/// The `--skills-dir` and `--packages-dir` of an `ExecStart=` value of the shape
/// `PROGRAM skill-runner serve --skills-dir A --packages-dir B`.
fn exec_dirs(value: &str) -> Option<(String, String)> {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    match tokens.as_slice() {
        [
            program,
            "skill-runner",
            "serve",
            "--skills-dir",
            skills,
            "--packages-dir",
            packages,
        ] if program.starts_with('/') && skills.starts_with('/') && packages.starts_with('/') => {
            Some((skills.to_string(), packages.to_string()))
        }
        _ => None,
    }
}

/// UNIT-2 and UNIT-3 for the runner service.
fn check_runner_service(units: &[UnitFile]) -> Vec<Violation> {
    let Some(unit) = units.iter().find(|u| u.name == RUNNER_SERVICE) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // UNIT-2: the account carries the control, so the unit adds no group.
    for entry in entries(unit, "SupplementaryGroups").filter(|e| !e.value.is_empty()) {
        out.push(violation(
            "UNIT-2",
            &at(unit, entry.line),
            &format!(
                "the runner unit has `SupplementaryGroups={}`, but the runner takes no group of any kind",
                entry.value
            ),
        ));
    }
    if unit.values("Service", "Group") != [RUNNER_ACCOUNT] {
        out.push(violation(
            "UNIT-2",
            &unit_at(unit),
            &format!(
                "the runner unit must have exactly `Group={RUNNER_ACCOUNT}`, found {:?}",
                unit.values("Service", "Group")
            ),
        ));
    }
    // UNIT-3: the baseline, then the two values that hold the configured directories.
    for (key, want) in RUNNER_UNIT_DIRECTIVES {
        let found = unit.values("Service", key);
        if found != [*want] {
            let shown = if found.is_empty() {
                "missing".to_string()
            } else {
                format!("{found:?}")
            };
            out.push(violation(
                "UNIT-3",
                &unit_at(unit),
                &format!("[Service] {key} must be exactly `{want}`, found {shown}"),
            ));
        }
    }
    for entry in entries(unit, "SupplementaryGroups") {
        out.push(violation(
            "UNIT-3",
            &at(unit, entry.line),
            "the runner unit must have no `SupplementaryGroups=` line",
        ));
    }
    match unit.values("Service", "ExecStart").as_slice() {
        [exec] => match exec_dirs(exec) {
            Some((skills, packages)) => {
                let want = format!("{skills} {packages}");
                if unit.values("Service", "BindReadOnlyPaths") != [want.as_str()] {
                    out.push(violation(
                        "UNIT-3",
                        &unit_at(unit),
                        &format!(
                            "[Service] BindReadOnlyPaths must be exactly `{want}`, the two directories of ExecStart, found {:?}",
                            unit.values("Service", "BindReadOnlyPaths")
                        ),
                    ));
                }
            }
            None => out.push(violation(
                "UNIT-3",
                &unit_at(unit),
                &format!(
                    "[Service] ExecStart must be `<absolute program> skill-runner serve --skills-dir <absolute> --packages-dir <absolute>`, found `{exec}`"
                ),
            )),
        },
        other => out.push(violation(
            "UNIT-3",
            &unit_at(unit),
            &format!("the runner unit must have exactly one ExecStart, found {}", other.len()),
        )),
    }
    out
}

/// UNIT-5, runner clause: the runner socket exists and listens on the runner path only.
fn check_runner_socket(units: &[UnitFile]) -> Vec<Violation> {
    let Some(unit) = units.iter().find(|u| u.name == RUNNER_SOCKET) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if unit.values("Socket", "ListenStream") != [RUNNER_SOCKET_PATH] {
        out.push(violation(
            "UNIT-5",
            &unit_at(unit),
            &format!(
                "the runner socket must listen on exactly `{RUNNER_SOCKET_PATH}`, found {:?}",
                unit.values("Socket", "ListenStream")
            ),
        ));
    }
    if unit.values("Socket", "SocketGroup") != [SOCKET_GROUP] {
        out.push(violation(
            "UNIT-5",
            &unit_at(unit),
            &format!("the runner socket must belong to `{SOCKET_GROUP}`"),
        ));
    }
    out
}

/// The non-comment lines of an installer outside heredoc bodies, with one-based line numbers.
fn script_lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut delimiter: Option<String> = None;
    for (i, line) in text.lines().enumerate() {
        if let Some(end) = &delimiter {
            if line.trim() == end {
                delimiter = None;
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((_, rest)) = line.split_once("<<") {
            let word: String = rest
                .trim_start_matches(['-', ' ', '\'', '"'])
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !word.is_empty() && !rest.starts_with('<') {
                delimiter = Some(word);
            }
        }
        out.push((i + 1, trimmed));
    }
    out
}

/// INSTALL-1, runner clause: the runner directories are resolved and created, both units are
/// placed, the daemon reloaded and the self-test run, all before the first command that
/// enables or starts the runner socket.
pub fn check_install_order(script: &str) -> Vec<Violation> {
    let lines = script_lines(script);
    let first = |pred: &dyn Fn(&str) -> bool| lines.iter().find(|(_, l)| pred(l)).map(|(n, _)| *n);
    let starts_socket = |l: &str| {
        l.contains("systemctl")
            && (l.contains(" enable") || l.contains(" start"))
            && l.contains(RUNNER_SOCKET)
    };
    let Some(enable) = first(&starts_socket) else {
        return vec![violation(
            "INSTALL-1",
            INSTALL_SCRIPT,
            "no line enables or starts the skill runner socket",
        )];
    };
    let before = |pred: &dyn Fn(&str) -> bool| lines.iter().any(|(n, l)| *n < enable && pred(l));
    let mut out = Vec::new();
    let mut need = |ok: bool, what: &str| {
        if !ok {
            out.push(violation(
                "INSTALL-1",
                &format!("{INSTALL_SCRIPT}:{enable}"),
                &format!("the skill runner socket is enabled before {what}"),
            ));
        }
    };
    need(
        before(&|l| l == "runner_dirs"),
        "the runner directories are read from `agent dirs`",
    );
    need(
        before(&|l| l.starts_with("make_dir") && l.contains("$runner_skills_dir")),
        "the configured skills directory is created",
    );
    need(
        before(&|l| l.starts_with("make_dir") && l.contains("$runner_packages_dir")),
        "the configured packages directory is created",
    );
    need(
        before(&|l| l.starts_with("place_unit") && l.contains(RUNNER_SOCKET)),
        "the runner socket unit is installed",
    );
    need(
        before(&|l| l.starts_with("place_unit") && l.contains(RUNNER_SERVICE)),
        "the runner service unit is installed",
    );
    let placed = lines
        .iter()
        .filter(|(n, l)| {
            *n < enable && l.starts_with("place_unit") && l.contains("domovoi-skill-runner")
        })
        .map(|(n, _)| *n)
        .max()
        .unwrap_or(0);
    need(
        lines
            .iter()
            .any(|(n, l)| *n > placed && *n < enable && l.contains("systemctl daemon-reload")),
        "the units are loaded with `systemctl daemon-reload`",
    );
    need(
        before(&|l| l.contains("agent selftest --installer")),
        "`agent selftest --installer` has passed",
    );
    out
}

#[cfg(test)]
#[path = "runner_rules_tests.rs"]
mod tests;
