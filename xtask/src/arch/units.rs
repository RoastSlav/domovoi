//! Systemd unit rules over `deploy/`: the unit parser and UNIT-2 (docker clause), UNIT-3
//! (helper unit), UNIT-4, UNIT-5, UNIT-7, UNIT-8 and UNIT-9, plus the entry point that runs them
//! and INSTALL-1 over the real tree.
//!
//! A unit is read as ordered `(section, key, value)` triples: a key may repeat, a trailing `\`
//! joins the next line, and a line starting with `;` or `#` is a comment.

use std::collections::BTreeSet;
use std::path::Path;

use super::Violation;
use super::compose::check_compose;
use super::identity_rules::violation;
use super::install_script::{self, InstallScript};
use super::rules::{
    COMPOSE_DIR, HELPER_UNIT, HELPER_UNIT_DEVICE_ALLOW, HELPER_UNIT_DIRECTIVES, INSTALL_SCRIPT,
    SELFTEST_UNIT, SELFTEST_UNIT_DIRECTIVES, SOCKET_GROUP, SOCKET_LISTEN_STREAMS, UNITS_DIR,
};

/// Whether a missing created-set block in the real `install.sh` is a violation. The unit rules
/// cannot check a path against a list that is not there, and the installer must publish it.
pub const REQUIRE_CREATED_SET: bool = true;

/// One `key=value` line of a unit, with the section it sits in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The `[Section]` the line is in; empty before the first header.
    pub section: String,
    /// The directive name.
    pub key: String,
    /// The value, trimmed; empty is a reset.
    pub value: String,
    /// One-based line number where the directive starts.
    pub line: usize,
}

/// A parsed unit file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFile {
    /// The file name, e.g. `domovoi-helper.socket`.
    pub name: String,
    /// Every directive in file order.
    pub entries: Vec<Entry>,
}

impl UnitFile {
    /// Parse `text` as the unit called `name`.
    pub fn parse(name: &str, text: &str) -> Self {
        Self {
            name: name.to_string(),
            entries: parse_unit(text),
        }
    }

    /// Every value of `key` in `section`, in order.
    pub fn values(&self, section: &str, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.section == section && e.key == key)
            .map(|e| e.value.as_str())
            .collect()
    }

    /// Every entry with `key`, in any section.
    fn keyed<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Entry> {
        self.entries.iter().filter(move |e| e.key == key)
    }
}

/// Parse unit text into ordered entries.
pub fn parse_unit(text: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut section = String::new();
    let mut lines = text.lines().enumerate();
    while let Some((index, raw)) = lines.next() {
        let first = raw.trim();
        if first.is_empty() || first.starts_with(';') || first.starts_with('#') {
            continue;
        }
        let mut joined = first.to_string();
        while joined.ends_with('\\') {
            joined.pop();
            for (_, next) in lines.by_ref() {
                let next = next.trim();
                if next.starts_with(';') || next.starts_with('#') {
                    continue;
                }
                joined.push(' ');
                joined.push_str(next);
                break;
            }
        }
        let joined = joined.trim();
        if let Some(header) = joined.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            section = header.trim().to_string();
        } else if let Some((key, value)) = joined.split_once('=') {
            entries.push(Entry {
                section: section.clone(),
                key: key.trim().to_string(),
                value: value.trim().to_string(),
                line: index + 1,
            });
        }
    }
    entries
}

fn at(unit: &UnitFile, line: usize) -> String {
    format!("{UNITS_DIR}/{}:{line}", unit.name)
}

fn unit_at(unit: &UnitFile) -> String {
    format!("{UNITS_DIR}/{}", unit.name)
}

/// Run every unit rule over `units`. `created` is the installer's created-set, or `None` when
/// the installer publishes none; with `require_created_set` that is a UNIT-7 violation when a
/// unit has a path directive to check.
pub fn check_units(
    units: &[UnitFile],
    created: Option<&BTreeSet<String>>,
    require_created_set: bool,
) -> Vec<Violation> {
    let mut out = Vec::new();
    for unit in units {
        out.extend(check_docker_groups(unit));
        out.extend(check_runtime_max(unit));
        out.extend(check_exec_paths(unit));
        if unit.name.ends_with(".socket") {
            out.extend(check_socket(unit));
        }
    }
    out.extend(check_helper_unit(units));
    out.extend(check_selftest_unit(units));
    out.extend(check_agent_user(units));
    out.extend(check_masks(units, created, require_created_set));
    out.extend(super::runner_rules::check_runner_units(units));
    out
}

/// UNIT-2, docker clause: no `Group=` or `SupplementaryGroups=` value names `docker`.
fn check_docker_groups(unit: &UnitFile) -> Vec<Violation> {
    let mut out = Vec::new();
    for key in ["Group", "SupplementaryGroups"] {
        for entry in unit.keyed(key) {
            let names_docker = entry
                .value
                .split(|c: char| c.is_whitespace() || c == ',')
                .any(|group| group.trim_start_matches('-') == "docker");
            if names_docker {
                out.push(violation(
                    "UNIT-2",
                    &at(unit, entry.line),
                    &format!("`{key}={}` puts the unit in the docker group", entry.value),
                ));
            }
        }
    }
    out
}

/// UNIT-3: the helper template unit carries the baseline, and no `User=` or
/// `SupplementaryGroups=`.
fn check_helper_unit(units: &[UnitFile]) -> Vec<Violation> {
    let Some(unit) = units.iter().find(|u| u.name == HELPER_UNIT) else {
        return vec![violation(
            "UNIT-3",
            &format!("{UNITS_DIR}/{HELPER_UNIT}"),
            "the helper unit is missing",
        )];
    };
    let mut out = Vec::new();
    for (key, want) in HELPER_UNIT_DIRECTIVES {
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
    if unit.values("Service", "DeviceAllow") != HELPER_UNIT_DEVICE_ALLOW {
        out.push(violation(
            "UNIT-3",
            &unit_at(unit),
            &format!(
                "[Service] DeviceAllow lines must be exactly {HELPER_UNIT_DEVICE_ALLOW:?}, found {:?}",
                unit.values("Service", "DeviceAllow")
            ),
        ));
    }
    for key in ["User", "SupplementaryGroups"] {
        for entry in unit.keyed(key) {
            out.push(violation(
                "UNIT-3",
                &at(unit, entry.line),
                &format!("the helper unit must have no `{key}=` line"),
            ));
        }
    }
    out
}

/// UNIT-3, self-test clause: the one-shot self-test unit carries its directives, runs before the
/// helper socket and after `nftables.service`, has no `User=` and no `RestrictNamespaces=`; and
/// the helper's own socket and template run after `nftables.service` too, so a reload cannot race
/// the table they guard.
fn check_selftest_unit(units: &[UnitFile]) -> Vec<Violation> {
    let after_nft = |unit: &UnitFile| {
        unit.values("Unit", "After")
            .iter()
            .any(|v| v.split_whitespace().any(|name| name == "nftables.service"))
    };
    let mut out = Vec::new();
    for name in ["domovoi-helper.socket", HELPER_UNIT] {
        if let Some(unit) = units.iter().find(|u| u.name == name)
            && !after_nft(unit)
        {
            out.push(violation(
                "UNIT-3",
                &unit_at(unit),
                "[Unit] After must name nftables.service",
            ));
        }
    }
    let Some(unit) = units.iter().find(|u| u.name == SELFTEST_UNIT) else {
        return vec![violation(
            "UNIT-3",
            &format!("{UNITS_DIR}/{SELFTEST_UNIT}"),
            "the self-test unit is missing",
        )];
    };
    for (key, want) in SELFTEST_UNIT_DIRECTIVES {
        let found = unit.values("Service", key);
        if found != [*want] {
            out.push(violation(
                "UNIT-3",
                &unit_at(unit),
                &format!("[Service] {key} must be exactly `{want}`, found {found:?}"),
            ));
        }
    }
    let before = unit.values("Unit", "Before");
    if !before
        .iter()
        .any(|v| v.split_whitespace().any(|n| n == "domovoi-helper.socket"))
    {
        out.push(violation(
            "UNIT-3",
            &unit_at(unit),
            "[Unit] Before must name domovoi-helper.socket",
        ));
    }
    if !after_nft(unit) {
        out.push(violation(
            "UNIT-3",
            &unit_at(unit),
            "[Unit] After must name nftables.service",
        ));
    }
    for key in ["User", "RestrictNamespaces"] {
        for entry in unit.keyed(key) {
            out.push(violation(
                "UNIT-3",
                &at(unit, entry.line),
                &format!("the self-test unit must have no `{key}=` line"),
            ));
        }
    }
    out
}

/// UNIT-4: `RuntimeMaxSec=` needs an explicit `Type=simple`.
fn check_runtime_max(unit: &UnitFile) -> Vec<Violation> {
    let Some(entry) = unit.keyed("RuntimeMaxSec").next() else {
        return Vec::new();
    };
    if unit.values("Service", "Type") == ["simple"] {
        return Vec::new();
    }
    vec![violation(
        "UNIT-4",
        &at(unit, entry.line),
        "`RuntimeMaxSec=` is set without `Type=simple`",
    )]
}

/// UNIT-5: a shipped socket's mode, owner, group and path.
fn check_socket(unit: &UnitFile) -> Vec<Violation> {
    let mut out = Vec::new();
    for (key, want) in [
        ("SocketMode", "0660"),
        ("SocketUser", "root"),
        ("SocketGroup", SOCKET_GROUP),
    ] {
        if unit.values("Socket", key) != [want] {
            out.push(violation(
                "UNIT-5",
                &unit_at(unit),
                &format!(
                    "[Socket] {key} must be exactly `{want}`, found {:?}",
                    unit.values("Socket", key)
                ),
            ));
        }
    }
    for entry in unit.keyed("ListenStream") {
        if !SOCKET_LISTEN_STREAMS.contains(&entry.value.as_str()) {
            out.push(violation(
                "UNIT-5",
                &at(unit, entry.line),
                &format!("`ListenStream={}` is not a known socket path", entry.value),
            ));
        }
    }
    if unit.keyed("ListenStream").next().is_none() {
        out.push(violation(
            "UNIT-5",
            &unit_at(unit),
            "the socket has no ListenStream",
        ));
    }
    out
}

/// UNIT-8, unit clause: the agent unit runs as `domovoi-agent`.
fn check_agent_user(units: &[UnitFile]) -> Vec<Violation> {
    let Some(unit) = units.iter().find(|u| u.name == "domovoi-agent.service") else {
        return vec![violation(
            "UNIT-8",
            &format!("{UNITS_DIR}/domovoi-agent.service"),
            "the agent unit is missing",
        )];
    };
    if unit.values("Service", "User") == [SOCKET_GROUP] {
        return Vec::new();
    }
    vec![violation(
        "UNIT-8",
        &unit_at(unit),
        &format!(
            "the agent unit must have exactly `User={SOCKET_GROUP}`, found {:?}",
            unit.values("Service", "User")
        ),
    )]
}

/// UNIT-9: the program of an exec line is not under a directory that may be `noexec`.
fn check_exec_paths(unit: &UnitFile) -> Vec<Violation> {
    let mut out = Vec::new();
    for key in ["ExecStart", "ExecStartPre", "ExecStopPost"] {
        for entry in unit.keyed(key) {
            let program = entry
                .value
                .trim_start_matches(['-', '@', '+', '!', ':'])
                .split_whitespace()
                .next()
                .unwrap_or_default();
            let under = ["/run", "/tmp", "/dev/shm"].iter().any(|dir| {
                program == *dir
                    || program
                        .strip_prefix(dir)
                        .is_some_and(|r| r.starts_with('/'))
            });
            if under {
                out.push(violation(
                    "UNIT-9",
                    &at(unit, entry.line),
                    &format!(
                        "`{key}={}` runs a program under /run, /tmp or /dev/shm",
                        entry.value
                    ),
                ));
            }
        }
    }
    out
}

/// The paths a mask or bind directive names, each with whether it carries a `-` prefix.
fn directive_paths(key: &str, value: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for token in value.split_whitespace() {
        let optional = token.starts_with('-');
        let token = token.trim_start_matches(['-', '+', '!', '~']);
        let segments: Vec<&str> = token.split(':').collect();
        let paths = match key {
            "BindReadOnlyPaths" => segments
                .iter()
                .take(2)
                .filter(|s| s.starts_with('/'))
                .copied()
                .collect::<Vec<_>>(),
            "TemporaryFileSystem" => segments.iter().take(1).copied().collect(),
            _ => vec![token],
        };
        for path in paths {
            let path = if path.len() > 1 {
                path.trim_end_matches('/')
            } else {
                path
            };
            out.push((path.to_string(), optional));
        }
    }
    out
}

/// UNIT-7: every plain mask, bind or writable path is created by the installer.
fn check_masks(
    units: &[UnitFile],
    created: Option<&BTreeSet<String>>,
    require_created_set: bool,
) -> Vec<Violation> {
    const KEYS: [&str; 4] = [
        "InaccessiblePaths",
        "BindReadOnlyPaths",
        "TemporaryFileSystem",
        "ReadWritePaths",
    ];
    let mut out = Vec::new();
    let mut reported_missing = false;
    for unit in units {
        for key in KEYS {
            for entry in unit.keyed(key) {
                let Some(created) = created else {
                    if require_created_set && !reported_missing {
                        reported_missing = true;
                        out.push(violation(
                            "UNIT-7",
                            &at(unit, entry.line),
                            "install.sh has no created-set block, so no path can be checked",
                        ));
                    }
                    continue;
                };
                for (path, optional) in directive_paths(key, &entry.value) {
                    let known = created.contains(&path);
                    if optional && known {
                        out.push(violation(
                            "UNIT-7",
                            &at(unit, entry.line),
                            &format!(
                                "`-{path}` is created by the installer, so it takes no `-` prefix"
                            ),
                        ));
                    } else if !optional && !known && path != "/var/lib" && path != "/run" {
                        out.push(violation(
                            "UNIT-7",
                            &at(unit, entry.line),
                            &format!("`{key}` names `{path}`, which the installer does not create"),
                        ));
                    }
                }
            }
        }
    }
    out
}

/// What `deploy/` and the installer hold.
struct Deploy {
    units: Vec<UnitFile>,
    compose: Vec<(String, String)>,
    script: Option<String>,
}

/// Load the shipped units, the compose files and the installer from `root`.
fn load(root: &Path) -> anyhow::Result<Deploy> {
    let mut units = Vec::new();
    let dir = root.join(UNITS_DIR);
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if name.ends_with(".service") || name.ends_with(".socket") {
                units.push(UnitFile::parse(&name, &std::fs::read_to_string(&path)?));
            }
        }
    }
    units.sort_by(|a, b| a.name.cmp(&b.name));
    let mut compose = Vec::new();
    let dir = root.join(COMPOSE_DIR);
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name.ends_with(".yml") || name.ends_with(".yaml") {
                compose.push((
                    format!("{COMPOSE_DIR}/{name}"),
                    std::fs::read_to_string(&path)?,
                ));
            }
        }
    }
    compose.sort();
    let script = root.join(INSTALL_SCRIPT);
    let script = if script.is_file() {
        Some(std::fs::read_to_string(script)?)
    } else {
        None
    };
    Ok(Deploy {
        units,
        compose,
        script,
    })
}

/// Run every unit, compose and installer rule over the tree under `root`.
pub fn check_deploy(root: &Path, require_created_set: bool) -> anyhow::Result<Vec<Violation>> {
    let Deploy {
        units,
        compose,
        script,
    } = load(root)?;
    let parsed: Option<InstallScript> = script.as_deref().map(install_script::parse);
    let created: Option<BTreeSet<String>> = parsed
        .as_ref()
        .and_then(|p| p.created.as_ref())
        .map(|paths| paths.iter().cloned().collect());
    let mut out = check_units(&units, created.as_ref(), require_created_set);
    out.extend(super::runner_rules::check_runner_present(&units));
    out.extend(check_compose(&compose));
    match &parsed {
        Some(parsed_script) => {
            out.extend(install_script::check(parsed_script, require_created_set));
            if let Some(text) = &script {
                out.extend(super::runner_rules::check_install_order(text));
            }
        }
        None if require_created_set => out.push(violation(
            "INSTALL-1",
            INSTALL_SCRIPT,
            "install.sh is missing",
        )),
        None => {}
    }
    Ok(out)
}

#[cfg(test)]
#[path = "units_tests.rs"]
mod tests;
