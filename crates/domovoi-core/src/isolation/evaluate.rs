//! Decides the runner-side checks (ST-05, ST-06, ST-13) from collected facts. Pure.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::probe::{AccessRecord, IsolationReport, MountRole, ProcStatus};
use crate::identity::evaluate_runner_groups;
use crate::ports::{Access, Probe};

/// Why the runner's startup facts are refused (ST-05, ST-06).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "fault", rename_all = "snake_case")]
pub enum RunnerFault {
    /// The runner holds groups besides its own.
    #[error("extra groups: {gids:?}")]
    ExtraGroups {
        /// The offending gids.
        gids: Vec<u32>,
    },
    /// The runner holds capabilities.
    #[error("capabilities held: effective {effective:#x}, bounding {bounding:#x}")]
    Capabilities {
        /// `CapEff`.
        effective: u64,
        /// `CapBnd`.
        bounding: u64,
    },
    /// The runner may gain privileges through `exec`.
    #[error("NoNewPrivs is not set")]
    NoNewPrivsUnset,
    /// `/proc/self/status` lacks a field or it does not parse.
    #[error("unreadable status field {field}")]
    Unreadable {
        /// The field.
        field: String,
    },
}

/// Reads the fields the runner checks need from the text of `/proc/self/status`.
pub fn parse_proc_status(text: &str) -> Result<ProcStatus, RunnerFault> {
    let field = |name: &str| -> Result<&str, RunnerFault> {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .map(str::trim)
            .ok_or_else(|| RunnerFault::Unreadable {
                field: name.to_owned(),
            })
    };
    let bad = |name: &str| RunnerFault::Unreadable {
        field: name.to_owned(),
    };
    let first_id = |name: &str| -> Result<u32, RunnerFault> {
        field(name)?
            .split_whitespace()
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| bad(name))
    };
    let caps = |name: &str| -> Result<u64, RunnerFault> {
        u64::from_str_radix(field(name)?, 16).map_err(|_| bad(name))
    };
    let groups = field("Groups")?
        .split_whitespace()
        .map(|v| v.parse().map_err(|_| bad("Groups")))
        .collect::<Result<Vec<u32>, _>>()?;
    Ok(ProcStatus {
        uid: first_id("Uid")?,
        gid: first_id("Gid")?,
        groups,
        cap_eff: caps("CapEff")?,
        cap_bnd: caps("CapBnd")?,
        no_new_privs: field("NoNewPrivs")? == "1",
    })
}

/// The runner's startup check: only its own group, no capabilities, `NoNewPrivs` set.
///
/// Returns every fault found.
#[must_use]
pub fn evaluate_runner_startup(status: &ProcStatus) -> Vec<RunnerFault> {
    let mut faults = Vec::new();
    if let Err(gids) = evaluate_runner_groups(&status.groups, status.gid) {
        faults.push(RunnerFault::ExtraGroups { gids });
    }
    if status.cap_eff != 0 || status.cap_bnd != 0 {
        faults.push(RunnerFault::Capabilities {
            effective: status.cap_eff,
            bounding: status.cap_bnd,
        });
    }
    if !status.no_new_privs {
        faults.push(RunnerFault::NoNewPrivsUnset);
    }
    faults
}

/// One failing point of the isolation report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationFault {
    /// The point, 1 to 7 (point 8 is informational and never fails).
    pub point: u8,
    /// What is wrong, in words.
    pub what: String,
}

fn fault(out: &mut Vec<IsolationFault>, point: u8, what: String) {
    out.push(IsolationFault { point, what });
}

/// `ENOENT` is required; `EACCES` means the mask failed and is called out as such.
fn require_hidden(out: &mut Vec<IsolationFault>, point: u8, records: &[AccessRecord]) {
    for r in records {
        match r.access {
            Access::NotFound => {}
            Access::Denied => fault(
                out,
                point,
                format!(
                    "{} ({:?}) gave EACCES, not ENOENT: the mask failed",
                    r.path.display(),
                    r.how
                ),
            ),
            other => fault(
                out,
                point,
                format!("{} ({:?}) is reachable: {other:?}", r.path.display(), r.how),
            ),
        }
    }
}

fn names_within(names: &[String], allowed: &[&str]) -> bool {
    names.iter().all(|n| allowed.contains(&n.as_str()))
}

/// Decides the eight-point report; returns every failing point.
///
/// `ENOENT` where a path must be hidden is the only pass: `EACCES` there is a failed mask. Point 8
/// (`/etc/shadow`) and the AppArmor label are informational and never fail.
#[must_use]
pub fn evaluate_isolation(report: &IsolationReport) -> Vec<IsolationFault> {
    let mut out = Vec::new();
    point_hidden(&mut out, report);
    point_listings(&mut out, report);
    point_writes(&mut out, report);
    point_home(&mut out, report);
    if report.proc_hidden.is_empty() {
        fault(&mut out, 6, "no /proc probe was collected".into());
    }
    require_hidden(&mut out, 6, &report.proc_hidden);
    point_status(&mut out, report);
    out.sort_by_key(|f| f.point);
    out
}

fn point_hidden(out: &mut Vec<IsolationFault>, report: &IsolationReport) {
    if report.connects.is_empty() {
        fault(out, 1, "no connect probe was collected".into());
    }
    require_hidden(out, 1, &report.connects);
    if report.hidden_state.is_empty() {
        fault(out, 2, "no state-directory probe was collected".into());
    }
    require_hidden(out, 2, &report.hidden_state);
}

fn point_listings(out: &mut Vec<IsolationFault>, report: &IsolationReport) {
    match &report.etc_listing {
        Some(names) if names.len() == 1 && names[0] == "skills" => {}
        other => fault(
            out,
            3,
            format!("/etc/domovoi lists {other:?}, not [\"skills\"]"),
        ),
    }
    match &report.var_lib_listing {
        Some(names) if names.len() == 1 && names[0] == "domovoi-packages" => {}
        other => fault(
            out,
            3,
            format!("/var/lib lists {other:?}, not [\"domovoi-packages\"]"),
        ),
    }
    require_hidden(out, 3, &report.config_files);
    let Some(run) = &report.run else {
        fault(out, 3, "/run was not inspected".into());
        return;
    };
    if !names_within(&run.run_entries, &["systemd"]) {
        fault(
            out,
            3,
            format!("/run holds {:?}, allowed only systemd", run.run_entries),
        );
    }
    if let Some(names) = &run.systemd_entries
        && !names_within(names, &["incoming"])
    {
        fault(
            out,
            3,
            format!("/run/systemd holds {names:?}, allowed only incoming"),
        );
    }
    if let Some(incoming) = &run.incoming
        && (!incoming.is_dir || incoming.uid != 0 || incoming.mode & 0o077 != 0)
    {
        fault(
            out,
            3,
            "/run/systemd/incoming is not a root-owned directory closed to group and other".into(),
        );
    }
}

fn point_writes(out: &mut Vec<IsolationFault>, report: &IsolationReport) {
    let dirs = [&report.dirs.skills_dir, &report.dirs.packages_dir];
    for dir in dirs {
        for how in [Probe::Create, Probe::Rename, Probe::Unlink] {
            let found = report
                .writes
                .iter()
                .find(|r| r.how == how && Path::new(&r.path) == dir.as_path());
            match found.map(|r| r.access) {
                Some(Access::ReadOnly) => {}
                Some(other) => fault(
                    out,
                    4,
                    format!("{how:?} in {} gave {other:?}, not EROFS", dir.display()),
                ),
                None => fault(out, 4, format!("no {how:?} probe for {}", dir.display())),
            }
        }
    }
    for r in report.writes.iter().filter(|r| r.how == Probe::Append) {
        if !matches!(r.access, Access::ReadOnly | Access::Denied) {
            fault(
                out,
                4,
                format!(
                    "append to {} gave {:?}, not EROFS or EACCES",
                    r.path.display(),
                    r.access
                ),
            );
        }
    }
}

fn point_home(out: &mut Vec<IsolationFault>, report: &IsolationReport) {
    for r in &report.homes {
        if !matches!(r.access, Access::Denied | Access::NotFound) {
            fault(
                out,
                5,
                format!("{} is reachable: {:?}", r.path.display(), r.access),
            );
        }
    }
    if report.run_user_exists {
        fault(out, 5, "/run/user exists".into());
    }
}

fn point_status(out: &mut Vec<IsolationFault>, report: &IsolationReport) {
    for f in evaluate_runner_startup(&report.status) {
        fault(out, 7, f.to_string());
    }
    if report.status.uid == 0 || report.status.gid == 0 {
        fault(out, 7, "the runner has uid or gid 0".into());
    }
    for role in [MountRole::SkillsBind, MountRole::PackagesBind] {
        match report.mounts.iter().find(|m| m.role == role) {
            Some(m) if m.read_only => {}
            Some(m) => fault(out, 7, format!("{} is not mounted ro", m.point.display())),
            None => fault(out, 7, format!("no {role:?} mount found")),
        }
    }
    let masks: Vec<_> = report
        .mounts
        .iter()
        .filter(|m| m.role == MountRole::Mask)
        .collect();
    if masks.len() < 3 {
        fault(
            out,
            7,
            format!("{} masks found, expected three", masks.len()),
        );
    }
    for m in masks.iter().filter(|m| !m.read_only) {
        fault(
            out,
            7,
            format!("mask {} is not mounted ro", m.point.display()),
        );
    }
}
