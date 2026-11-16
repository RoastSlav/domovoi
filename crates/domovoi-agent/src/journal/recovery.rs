//! Orphan recovery after a restart: identify, terminate, and record abandoned children.
//!
//! Emptiness is only concluded from a complete inspection. A process this agent cannot read
//! (permission, I/O, a malformed `/proc` entry) makes the group `Unknown`, never `Empty`.

use domovoi_core::ports::Clock;
use jiff::SignedDuration;
use uuid::Uuid;

use super::Journal;
use super::append::append_locked;
use super::record::Record;
use super::store::{Phase, phase_locked};

/// The identity of a live process, as `/proc/<pid>/stat` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProcessStat {
    /// Field 3: the one-letter process state; `Z` means a zombie, which no longer runs.
    pub(super) state: char,
    /// Field 22: clock ticks since boot.
    pub(super) start_time: u64,
    /// Field 5: process-group id.
    pub(super) pgrp: u32,
    /// Field 6: session id.
    pub(super) session: u32,
}

/// What inspecting one process established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProcessLookup {
    /// The process exists and its stat parsed.
    Running(ProcessStat),
    /// The process does not exist.
    Gone,
    /// The process exists but could not be read.
    Denied,
    /// The read failed for another reason (for example a transient I/O error).
    Failed,
    /// The entry exists but could not be parsed.
    Malformed,
}

/// Why a `/proc` enumeration did not complete.
#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub(super) struct InspectError {
    reason: String,
}

impl InspectError {
    pub(super) fn new(err: std::io::Error) -> Self {
        Self {
            reason: err.to_string(),
        }
    }
}

/// A signal this agent may send to a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Signal {
    Term,
    Kill,
}

/// The result of a group signal.
///
/// Only the Linux target can send signals; on other targets only `Failed` is reachable, and
/// the other variants exist so the inspection seam has one result type everywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SignalOutcome {
    Sent,
    /// The group no longer exists.
    Missing,
    Denied,
    Failed,
}

/// The injected seam for `/proc` inspection and group signals.
pub(super) trait ProcessInspector: Send + Sync {
    fn lookup(&self, pid: u32) -> ProcessLookup;
    fn list(&self) -> Result<Vec<u32>, InspectError>;
    fn group_exists(&self, pgid: u32) -> Option<bool>;
    fn signal(&self, pgid: u32, signal: Signal) -> SignalOutcome;
}

/// The real Linux inspector.
pub(super) struct ProcInspector;

impl ProcessInspector for ProcInspector {
    fn lookup(&self, pid: u32) -> ProcessLookup {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(text) => match parse_stat(&text) {
                Some(stat) => ProcessLookup::Running(stat),
                None => ProcessLookup::Malformed,
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => ProcessLookup::Gone,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => ProcessLookup::Denied,
            Err(_) => ProcessLookup::Failed,
        }
    }

    fn list(&self) -> Result<Vec<u32>, InspectError> {
        let entries = std::fs::read_dir("/proc").map_err(InspectError::new)?;
        let mut pids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(InspectError::new)?;
            if let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            {
                pids.push(pid);
            }
        }
        Ok(pids)
    }

    fn group_exists(&self, pgid: u32) -> Option<bool> {
        kernel_group_exists(pgid)
    }

    fn signal(&self, pgid: u32, signal: Signal) -> SignalOutcome {
        sys_signal(pgid, signal)
    }
}

fn parse_stat(text: &str) -> Option<ProcessStat> {
    let rest = text.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    Some(ProcessStat {
        state: fields.first()?.chars().next()?,
        pgrp: fields.get(2)?.parse().ok()?,
        session: fields.get(3)?.parse().ok()?,
        start_time: fields.get(19)?.parse().ok()?,
    })
}

#[cfg(target_os = "linux")]
fn kernel_group_exists(pgid: u32) -> Option<bool> {
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    match kill(Pid::from_raw(-(pgid as i32)), None) {
        Ok(()) => Some(true),
        Err(Errno::EPERM) => Some(true),
        Err(Errno::ESRCH) => Some(false),
        Err(_) => None,
    }
}

#[cfg(not(target_os = "linux"))]
fn kernel_group_exists(_pgid: u32) -> Option<bool> {
    None
}

#[cfg(target_os = "linux")]
fn sys_signal(pgid: u32, signal: Signal) -> SignalOutcome {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal as UnixSignal, killpg};
    use nix::unistd::Pid;
    let signal = match signal {
        Signal::Term => UnixSignal::SIGTERM,
        Signal::Kill => UnixSignal::SIGKILL,
    };
    match killpg(Pid::from_raw(pgid as i32), signal) {
        Ok(()) => SignalOutcome::Sent,
        Err(Errno::ESRCH) => SignalOutcome::Missing,
        Err(Errno::EPERM) => SignalOutcome::Denied,
        Err(_) => SignalOutcome::Failed,
    }
}

#[cfg(not(target_os = "linux"))]
fn sys_signal(_pgid: u32, _signal: Signal) -> SignalOutcome {
    SignalOutcome::Failed
}

/// One started execution left by a previous agent instance.
pub(super) struct StartedExecution {
    execution_id: Uuid,
    pid: u32,
    start_time: u64,
    pgid: u32,
    session: u32,
}

/// What a group inspection established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupState {
    NonEmpty,
    Empty,
    Unknown,
}

pub(super) fn started_without_finished(records: &[Record]) -> Vec<StartedExecution> {
    let mut started = Vec::new();
    let mut resolved = std::collections::BTreeSet::new();
    for record in records {
        match record {
            Record::Started {
                execution_id,
                pid,
                proc_start_time,
                pgid,
                session,
                ..
            } => started.push(StartedExecution {
                execution_id: *execution_id,
                pid: *pid,
                start_time: *proc_start_time,
                pgid: *pgid,
                session: *session,
            }),
            Record::Finished { execution_id, .. } | Record::Abandoned { execution_id, .. } => {
                resolved.insert(*execution_id);
            }
            _ => {}
        }
    }
    started
        .into_iter()
        .filter(|execution| !resolved.contains(&execution.execution_id))
        .collect()
}

/// Reads one process with the real inspector.
pub(super) fn process_stat(pid: u32) -> Option<ProcessStat> {
    match ProcInspector.lookup(pid) {
        ProcessLookup::Running(stat) => Some(stat),
        _ => None,
    }
}

/// Whether a group still has a live member, using the real inspector. Test-only.
#[cfg(all(test, target_os = "linux"))]
pub(super) fn group_state(pgid: u32, session: u32) -> GroupState {
    group_state_with(&ProcInspector, pgid, session)
}

/// Whether a process group still has a live (non-zombie) member in `session`.
///
/// A zombie cannot run code and is reaped by its parent or init, so it is not a live member.
/// The group is `Empty` only when the kernel says it does not exist, or when a complete scan
/// finds no live member; any unreadable or unparsable entry makes it `Unknown`.
pub(super) fn group_state_with(
    inspector: &dyn ProcessInspector,
    pgid: u32,
    session: u32,
) -> GroupState {
    let exists = inspector.group_exists(pgid);
    // Even a gone group needs a complete enumeration: an unreadable /proc is not evidence.
    let pids = match inspector.list() {
        Ok(pids) => pids,
        Err(err) => {
            tracing::warn!(%err, pgid, "could not enumerate processes for group recovery");
            return GroupState::Unknown;
        }
    };
    match exists {
        Some(false) => return GroupState::Empty,
        None => return GroupState::Unknown,
        Some(true) => {}
    }
    let mut live = false;
    let mut incomplete = false;
    for pid in pids {
        match inspector.lookup(pid) {
            ProcessLookup::Running(stat)
                if stat.state != 'Z'
                    && stat.pgrp == pgid
                    && (session == 0 || stat.session == session) =>
            {
                live = true;
            }
            ProcessLookup::Running(_) | ProcessLookup::Gone => {}
            ProcessLookup::Denied | ProcessLookup::Failed | ProcessLookup::Malformed => {
                incomplete = true;
            }
        }
    }
    if live {
        GroupState::NonEmpty
    } else if incomplete {
        GroupState::Unknown
    } else {
        GroupState::Empty
    }
}

/// Polls until the group is empty, bounded by the fake/real clock.
async fn wait_empty(
    inspector: &dyn ProcessInspector,
    clock: &dyn Clock,
    pgid: u32,
    session: u32,
) -> bool {
    for _ in 0..500 {
        match group_state_with(inspector, pgid, session) {
            GroupState::Empty => return true,
            GroupState::Unknown => return false,
            GroupState::NonEmpty => {}
        }
        let now = clock.now();
        let next = now
            .saturating_add(SignedDuration::from_millis(100))
            .unwrap_or(now);
        clock.sleep_until(next).await;
        // With a fake clock the sleep returns at once; let the signal be delivered.
        tokio::task::yield_now().await;
    }
    matches!(
        group_state_with(inspector, pgid, session),
        GroupState::Empty
    )
}

impl Journal {
    /// Terminates process groups left running by a previous agent instance.
    ///
    /// The original leader must still be the recorded process, or must have exited with a
    /// persisted group-leader identity. A group is signalled only when its identity is
    /// established, and `abandoned` is written only after a complete inspection proves the
    /// group is empty.
    pub async fn recover_orphans(&self, clock: &dyn Clock) {
        self.recover_orphans_with(clock, &ProcInspector).await;
    }

    /// [`Journal::recover_orphans`] with an injectable inspector. Test-only.
    pub(super) async fn recover_orphans_with(
        &self,
        clock: &dyn Clock,
        inspector: &dyn ProcessInspector,
    ) {
        let unresolved = match self.lock() {
            Ok(state) if state.fenced.is_none() => started_without_finished(&state.records),
            _ => return,
        };
        for execution in unresolved {
            let mut already_empty = false;
            let (pgid, session) = match inspector.lookup(execution.pid) {
                ProcessLookup::Running(stat) if stat.start_time == execution.start_time => {
                    // The runner starts every child as its own group leader, so the PID is
                    // the group id. The start time proves this is the original child.
                    (
                        execution.pid,
                        if execution.session == 0 {
                            stat.session
                        } else {
                            execution.session
                        },
                    )
                }
                ProcessLookup::Running(_) => {
                    // A reused PID, or a process whose recorded identity no longer matches.
                    continue;
                }
                ProcessLookup::Gone => {
                    // The leader has already exited. Only a persisted group-leader identity
                    // can authorize a signal; a legacy record without one stays unresolved.
                    if execution.pgid == 0
                        || execution.pgid != execution.pid
                        || execution.session == 0
                    {
                        continue;
                    }
                    match group_state_with(inspector, execution.pgid, execution.session) {
                        GroupState::Empty => {
                            already_empty = true;
                            (execution.pgid, execution.session)
                        }
                        GroupState::NonEmpty => (execution.pgid, execution.session),
                        GroupState::Unknown => {
                            tracing::warn!(
                                pid = execution.pid,
                                "the orphaned process group's identity is unknown; leaving it unresolved"
                            );
                            continue;
                        }
                    }
                }
                ProcessLookup::Denied | ProcessLookup::Failed | ProcessLookup::Malformed => {
                    tracing::warn!(
                        pid = execution.pid,
                        "the orphaned process could not be inspected; leaving it unresolved"
                    );
                    continue;
                }
            };
            if !already_empty {
                match inspector.signal(pgid, Signal::Term) {
                    SignalOutcome::Sent | SignalOutcome::Missing => {}
                    SignalOutcome::Denied | SignalOutcome::Failed => {
                        tracing::warn!(pid = execution.pid, "could not send SIGTERM to an orphan");
                        continue;
                    }
                }
                let now = clock.now();
                let deadline = now
                    .saturating_add(SignedDuration::from_secs(2))
                    .unwrap_or(now);
                clock.sleep_until(deadline).await;

                match group_state_with(inspector, pgid, session) {
                    GroupState::Empty => {}
                    GroupState::NonEmpty => {
                        match inspector.signal(pgid, Signal::Kill) {
                            SignalOutcome::Sent | SignalOutcome::Missing => {}
                            SignalOutcome::Denied | SignalOutcome::Failed => {
                                tracing::warn!(
                                    pid = execution.pid,
                                    "could not send SIGKILL to an orphan"
                                );
                                continue;
                            }
                        }
                        if !wait_empty(inspector, clock, pgid, session).await {
                            tracing::warn!(
                                pid = execution.pid,
                                "the orphaned group is not empty after SIGKILL; leaving it unresolved"
                            );
                            continue;
                        }
                    }
                    GroupState::Unknown => {
                        tracing::warn!(
                            pid = execution.pid,
                            "the orphaned process group's identity is unknown; leaving it unresolved"
                        );
                        continue;
                    }
                }
            }
            // Only a complete inspection that establishes emptiness may abandon.
            if group_state_with(inspector, pgid, session) != GroupState::Empty {
                tracing::warn!(
                    pid = execution.pid,
                    "the orphaned group could not be proven empty; leaving it unresolved"
                );
                continue;
            }
            match self.lock() {
                Ok(mut state)
                    if state.fenced.is_none()
                        && phase_locked(&state, execution.execution_id) == Some(Phase::Started) =>
                {
                    let record = Record::Abandoned {
                        format: Record::FORMAT,
                        execution_id: execution.execution_id,
                        pid: execution.pid,
                        reason: "the agent restarted while the process was alive".to_owned(),
                    };
                    if let Err(err) = append_locked(&mut state, &record) {
                        tracing::error!(%err, "could not record an abandoned execution");
                    }
                }
                Ok(_) => tracing::warn!(
                    effect = %execution.execution_id,
                    "the journal was fenced or the execution resolved before abandonment"
                ),
                Err(err) => tracing::error!(%err, "could not record an abandoned execution"),
            }
        }
    }
}
