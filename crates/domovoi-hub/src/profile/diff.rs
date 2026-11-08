//! What changed between two profile snapshots of the same host, one line each.
//!
//! The lines go into the note's `## Recent changes` so a person and the model can tell
//! what is new since the machine was last normal. Only the parts a profile actually keeps
//! are compared: a value that was never collected cannot look like a change.

use jiff::Timestamp;

use super::HostProfile;
use super::ProfileFacts;
use super::collect::CONTAINER_TITLE;
use super::collect::LISTENER_TITLE;

/// The most changes a profile keeps: enough for a person to scan, and the note is cut anyway.
pub const MAX_CHANGES: usize = 20;

/// Bytes in one GiB: memory is reported in the unit a person reads.
const GIB: u64 = 1024 * 1024 * 1024;

/// The differences between the `old` and `new` snapshots of one host, in a stable order:
/// containers first, then listening sockets and read skills, then the machine facts.
/// Identical profiles give no lines.
#[must_use]
pub fn changes(old: &HostProfile, new: &HostProfile) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    if collected(old, CONTAINER_TITLE) && collected(new, CONTAINER_TITLE) {
        for line in container_lines(old, new) {
            lines.push(line);
        }
    }
    if collected(old, LISTENER_TITLE) && collected(new, LISTENER_TITLE) {
        for line in named_lines("listening socket", &old.listeners, &new.listeners) {
            lines.push(line);
        }
    }
    for line in named_lines("read skill", &old.skills, &new.skills) {
        lines.push(line);
    }
    for line in machine_lines(&old.facts, &new.facts) {
        lines.push(line);
    }
    lines
}

/// The changes between `previous` and `profile`, each dated with `at`, newest first, then
/// the changes `previous` already kept, capped at [`MAX_CHANGES`].
pub fn recent_changes(
    previous: &Option<HostProfile>,
    at: Timestamp,
    profile: &HostProfile,
) -> Vec<String> {
    let fresh: Vec<String> = match previous {
        None => vec![],
        Some(older) => changes(older, profile)
            .iter()
            .map(|line| dated(at, line))
            .collect(),
    };
    let kept = match previous {
        None => vec![],
        Some(older) => older.changes.clone(),
    };
    let merged: Vec<String> = fresh.into_iter().chain(kept).take(MAX_CHANGES).collect();
    merged
}

/// A change line with the date it was collected on, so a reader knows when it happened.
fn dated(at: Timestamp, line: &str) -> String {
    format!("{}: {}", at.strftime("%Y-%m-%d %H:%M UTC"), line)
}

/// Whether `profile` has that section answered: a tool that did not answer leaves an empty
/// list behind, and comparing one of those against the previous profile would report the
/// whole inventory as removed, so those parts are only compared when both answered.
fn collected(profile: &HostProfile, title: &str) -> bool {
    profile
        .sections
        .iter()
        .any(|section| section.title == title && section.ok)
}

/// One difference line per container that came, went or changed image.
fn container_lines(old: &HostProfile, new: &HostProfile) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for unit in new.containers.iter() {
        match old.containers.iter().find(|other| other.name == unit.name) {
            None => lines.push(format!("container {} added", quoted(unit.name.as_str()))),
            Some(other) => {
                if other.image != unit.image {
                    lines.push(format!(
                        "container {} image changed from {} to {}",
                        quoted(unit.name.as_str()),
                        quoted(other.image.as_str()),
                        quoted(unit.image.as_str())
                    ));
                }
            }
        }
    }
    for unit in old.containers.iter() {
        if new
            .containers
            .iter()
            .find(|other| other.name == unit.name)
            .is_none()
        {
            lines.push(format!("container {} removed", quoted(unit.name.as_str())));
        }
    }
    lines
}

/// Lines for the names `new` has and `old` has not, and the ones it lost.
fn named_lines(noun: &str, old: &[String], new: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for name in new.iter().filter(|name| !old.contains(name)) {
        lines.push(format!("{} {} added", noun, quoted(name.as_str())));
    }
    for name in old.iter().filter(|name| !new.contains(name)) {
        lines.push(format!("{} {} removed", noun, quoted(name.as_str())));
    }
    lines
}

/// The machine facts the hub refreshes on: the operating system, the kernel, memory and
/// the number of logical CPUs.
fn machine_lines(old: &ProfileFacts, new: &ProfileFacts) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    if old.os != new.os {
        lines.push(format!(
            "os changed from {} to {}",
            quoted(old.os.as_str()),
            quoted(new.os.as_str())
        ));
    }
    if old.kernel != new.kernel {
        lines.push(format!(
            "kernel changed from {} to {}",
            quoted(old.kernel.as_str()),
            quoted(new.kernel.as_str())
        ));
    }
    if old.mem_total_bytes != new.mem_total_bytes {
        lines.push(format!(
            "memory changed from {} GiB to {} GiB",
            tenths_of_gib(old.mem_total_bytes),
            tenths_of_gib(new.mem_total_bytes)
        ));
    }
    if old.cpu_count != new.cpu_count {
        lines.push(format!(
            "cpu count changed from {} to {}",
            old.cpu_count, new.cpu_count
        ));
    }
    lines
}

/// A name or value in double quotes, so a line says where it ends.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name)
}

/// Memory as GiB with one decimal, so a change smaller than a whole gigabyte still says
/// something: 32.2 GiB and 32.8 GiB are not both printed as 32.
fn tenths_of_gib(bytes: u64) -> String {
    let tenths = bytes * 10 / GIB;
    format!("{}.{:01}", tenths / 10, tenths % 10)
}
