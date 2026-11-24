//! `domovoi helper repair`: the owner-confirmed local fix for a wrong mode or owner.
//!
//! The helper never repairs at start; this is a separate command. It changes only the paths a
//! fresh ST-10 and ST-11 run lists, and only their mode and ownership: never content, never a
//! creation or a deletion. It needs root, a terminal and the typed word `repair`. Nothing outside
//! `crates/domovoi/src/cmd/helper.rs` calls it.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, IsTerminal, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use domovoi_core::identity::{
    FaultKind, HostDirs, PathFacts, PathFault, PathId, PathKind, RefusalToken,
};
use domovoi_core::ports::HostInspect;
use nix::fcntl::OFlag;
use nix::sys::stat::{Mode, fchmod};
use nix::unistd::{Gid, Uid, fchown};

use crate::selftest::{render_fault, table_faults};

/// The word the owner must type.
pub const CONFIRM_WORD: &str = "repair";

/// What a repair does to one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Set the permission bits.
    Chmod(u32),
    /// Set the owner and group.
    Chown {
        /// The owner uid.
        uid: u32,
        /// The owner gid.
        gid: u32,
    },
    /// Both of the above.
    Both {
        /// The permission bits.
        mode: u32,
        /// The owner uid.
        uid: u32,
        /// The owner gid.
        gid: u32,
    },
}

impl Change {
    fn text(self) -> String {
        match self {
            Self::Chmod(mode) => format!("chmod {mode:04o}"),
            Self::Chown { uid, gid } => format!("chown {uid}:{gid}"),
            Self::Both { mode, uid, gid } => format!("chmod {mode:04o} and chown {uid}:{gid}"),
        }
    }
}

/// One path the plan changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairEntry {
    /// The path.
    pub path: PathBuf,
    /// What it is now.
    pub found: PathFacts,
    /// What the path table says it must be.
    pub expected: PathFacts,
    /// The change that makes it so.
    pub change: Change,
}

/// Why a fault is not repaired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefuseReason {
    /// The path is a symlink; a repair never follows or replaces one.
    Symlink,
    /// The path is the wrong kind of object; fixing that means replacing it.
    WrongType,
    /// The path is absent; a repair never creates anything.
    Missing,
    /// The path cannot be inspected, so nothing is known to change.
    Unreadable,
    /// The fault belongs to a check this command does not cover (the socket node).
    OutOfScope,
}

impl RefuseReason {
    const fn text(self) -> &'static str {
        match self {
            Self::Symlink => "is a symlink: remove or replace it yourself",
            Self::WrongType => "is the wrong type of object: replace it yourself",
            Self::Missing => "is missing: run the installer or `domovoi helper init`",
            Self::Unreadable => "cannot be inspected",
            Self::OutOfScope => "is not covered by repair",
        }
    }
}

/// What a repair would do and what it refuses to do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepairPlan {
    /// The paths whose mode or owner would change.
    pub entries: Vec<RepairEntry>,
    /// Faults that stay for the owner to fix by hand.
    pub refused: Vec<(PathBuf, RefuseReason)>,
}

/// Plans the repair of `faults` (the faults of ST-10 and ST-11). Pure.
///
/// A symlink, a wrong type, a missing or unreadable path and the socket node are refused; the
/// rest are a chmod, a chown or both, whichever makes the facts match.
#[must_use]
pub fn plan_repair(faults: &[PathFault]) -> RepairPlan {
    let mut plan = RepairPlan::default();
    let mut seen: Vec<&Path> = Vec::new();
    for fault in faults {
        if seen.contains(&fault.path.as_path()) {
            continue;
        }
        seen.push(&fault.path);
        let refuse = |plan: &mut RepairPlan, why| plan.refused.push((fault.path.clone(), why));
        let Some(found) = &fault.found else {
            let why = if fault.why == FaultKind::Missing {
                RefuseReason::Missing
            } else {
                RefuseReason::Unreadable
            };
            refuse(&mut plan, why);
            continue;
        };
        if fault.id == PathId::HelperSocket {
            refuse(&mut plan, RefuseReason::OutOfScope);
        } else if found.symlink {
            refuse(&mut plan, RefuseReason::Symlink);
        } else if found.kind.is_none() || found.kind != fault.expected.kind {
            refuse(&mut plan, RefuseReason::WrongType);
        } else {
            let mode = (found.mode & 0o7777 != fault.expected.mode).then_some(fault.expected.mode);
            let owner = (found.uid != fault.expected.uid || found.gid != fault.expected.gid)
                .then_some((fault.expected.uid, fault.expected.gid));
            let change = match (mode, owner) {
                (Some(mode), Some((uid, gid))) => Change::Both { mode, uid, gid },
                (Some(mode), None) => Change::Chmod(mode),
                (None, Some((uid, gid))) => Change::Chown { uid, gid },
                (None, None) => continue,
            };
            plan.entries.push(RepairEntry {
                path: fault.path.clone(),
                found: found.clone(),
                expected: fault.expected.clone(),
                change,
            });
        }
    }
    plan
}

fn facts_line(f: &PathFacts) -> String {
    format!("mode {:04o} owner {}:{}", f.mode & 0o7777, f.uid, f.gid)
}

/// The plan as the text the owner reads before typing the word.
#[must_use]
pub fn render_plan(plan: &RepairPlan) -> String {
    let mut out = String::new();
    if plan.entries.is_empty() {
        out.push_str("Nothing can be repaired.\n");
    } else {
        out.push_str("These paths will change (mode and ownership only):\n");
    }
    for e in &plan.entries {
        let _ = writeln!(
            out,
            "  {}: found {}; expected {}; will {}",
            e.path.display(),
            facts_line(&e.found),
            facts_line(&e.expected),
            e.change.text()
        );
    }
    for (path, why) in &plan.refused {
        let _ = writeln!(out, "  {} {}", path.display(), why.text());
    }
    out
}

/// Asks the owner to confirm a plan.
pub trait Confirm {
    /// Whether confirmation can be asked at all; false without a terminal.
    fn has_terminal(&self) -> bool {
        true
    }

    /// Shows `plan` and returns whether the owner confirmed it.
    fn confirmed(&self, plan: &RepairPlan) -> bool;
}

/// Whether `line` is the confirmation word, and only that.
#[must_use]
pub fn is_confirmation(line: &str) -> bool {
    line.trim() == CONFIRM_WORD
}

/// Confirms on the terminal: prints the plan and accepts only the typed word `repair`.
///
/// With stdin not a terminal it refuses outright; there is no flag that skips the question.
#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalConfirm;

impl Confirm for TerminalConfirm {
    fn has_terminal(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    fn confirmed(&self, plan: &RepairPlan) -> bool {
        let mut out = std::io::stdout();
        let asked = write!(
            out,
            "{}\nType `{CONFIRM_WORD}` to apply this plan: ",
            render_plan(plan)
        )
        .and_then(|()| out.flush());
        if asked.is_err() {
            return false;
        }
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).is_ok() && is_confirmation(&line)
    }
}

/// The operations a repair performs, so tests can watch them.
pub trait RepairOps {
    /// Whether this process is root.
    fn is_root(&self) -> bool;
    /// Applies one entry: mode and ownership of that path only.
    fn apply(&self, entry: &RepairEntry) -> Result<(), String>;
}

/// The real [`RepairOps`]: opens the path without following a symlink and uses `fchmod` and
/// `fchown` on the descriptor, so a path swapped for a link after planning is not followed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnixRepairOps;

impl RepairOps for UnixRepairOps {
    fn is_root(&self) -> bool {
        nix::unistd::geteuid().is_root()
    }

    fn apply(&self, entry: &RepairEntry) -> Result<(), String> {
        let file: File = OpenOptions::new()
            .read(true)
            .custom_flags(OFlag::O_NOFOLLOW.bits())
            .open(&entry.path)
            .map_err(|e| format!("cannot open: {e}"))?;
        let meta = file.metadata().map_err(|e| format!("cannot stat: {e}"))?;
        let kind_ok = match entry.found.kind {
            Some(PathKind::Dir) => meta.is_dir(),
            Some(PathKind::File) => meta.is_file(),
            _ => false,
        };
        if !kind_ok || meta.uid() != entry.found.uid || meta.gid() != entry.found.gid {
            return Err("changed since it was inspected".to_owned());
        }
        let (mode, owner) = match entry.change {
            Change::Chmod(mode) => (Some(mode), None),
            Change::Chown { uid, gid } => (None, Some((uid, gid))),
            Change::Both { mode, uid, gid } => (Some(mode), Some((uid, gid))),
        };
        if let Some((uid, gid)) = owner {
            fchown(&file, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid)))
                .map_err(|e| format!("chown failed: {e}"))?;
        }
        if let Some(mode) = mode {
            fchmod(&file, Mode::from_bits_truncate(mode as _))
                .map_err(|e| format!("chmod failed: {e}"))?;
        }
        Ok(())
    }
}

/// How an attempt to repair ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairOutcome {
    /// Not root; nothing changed.
    NotRoot,
    /// No terminal to ask on; nothing changed. Carries `confirmation_requires_terminal`.
    NoTerminal(RefusalToken),
    /// The plan has no entry to change.
    NothingToDo,
    /// The owner did not type the word; nothing changed.
    Declined,
    /// The plan was applied entry by entry.
    Applied {
        /// Paths changed.
        changed: Vec<PathBuf>,
        /// Paths whose change failed, with the reason.
        failed: Vec<(PathBuf, String)>,
    },
}

/// Applies `plan` after the owner confirms it.
///
/// Order: root check, terminal check, nothing-to-do, confirmation, then one change per entry.
/// A refusal before the last step touches nothing.
pub fn apply_repair(
    plan: &RepairPlan,
    confirm: &dyn Confirm,
    ops: &dyn RepairOps,
) -> RepairOutcome {
    if !ops.is_root() {
        return RepairOutcome::NotRoot;
    }
    if !confirm.has_terminal() {
        return RepairOutcome::NoTerminal(RefusalToken::ConfirmationRequiresTerminal);
    }
    if plan.entries.is_empty() {
        return RepairOutcome::NothingToDo;
    }
    if !confirm.confirmed(plan) {
        return RepairOutcome::Declined;
    }
    let mut changed = Vec::new();
    let mut failed = Vec::new();
    for entry in &plan.entries {
        match ops.apply(entry) {
            Ok(()) => changed.push(entry.path.clone()),
            Err(why) => failed.push((entry.path.clone(), why)),
        }
    }
    RepairOutcome::Applied { changed, failed }
}

/// A whole repair run: what was planned, what happened and what the re-check still finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairReport {
    /// The plan made from the faults found.
    pub plan: RepairPlan,
    /// How applying it ended.
    pub outcome: RepairOutcome,
    /// The faults ST-10 and ST-11 still find afterwards; empty when the host matches the table.
    pub remaining: Vec<PathFault>,
}

/// Why a repair run could not start.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepairError {
    /// The `domovoi-agent` account cannot be resolved, so ownership cannot be decided.
    #[error("the domovoi-agent account does not exist")]
    AgentAccountMissing,
}

/// Finds the faults of ST-10 and ST-11, plans, applies after confirmation, then runs both checks
/// again and reports what is left.
pub fn run_repair(
    inspect: &dyn HostInspect,
    dirs: &HostDirs,
    confirm: &dyn Confirm,
    ops: &dyn RepairOps,
) -> Result<RepairReport, RepairError> {
    let faults = table_faults(inspect, dirs).ok_or(RepairError::AgentAccountMissing)?;
    let plan = plan_repair(&faults.all());
    let outcome = apply_repair(&plan, confirm, ops);
    let remaining = table_faults(inspect, dirs)
        .map(|f| f.all())
        .unwrap_or_default();
    Ok(RepairReport {
        plan,
        outcome,
        remaining,
    })
}

/// The faults left after a repair, one line each, or a line saying there are none.
#[must_use]
pub fn render_remaining(remaining: &[PathFault]) -> String {
    if remaining.is_empty() {
        return "ST-10 and ST-11 now pass.\n".to_owned();
    }
    let mut out = String::from("Still wrong after the repair:\n");
    for fault in remaining {
        let _ = writeln!(out, "  {}", render_fault(fault));
    }
    out
}
