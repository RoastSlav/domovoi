//! Orphan recovery after a restart: identify, terminate, and record abandoned children.

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
    /// Field 22: clock ticks since boot.
    pub(super) start_time: u64,
    /// Field 5: process-group id.
    pub(super) pgrp: u32,
    /// Field 6: session id.
    pub(super) session: u32,
}

/// One started execution left by a previous agent instance.
pub(super) struct StartedExecution {
    execution_id: Uuid,
    pid: u32,
    start_time: u64,
    session: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupState {
    Alive,
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
                session,
                ..
            } => started.push(StartedExecution {
                execution_id: *execution_id,
                pid: *pid,
                start_time: *proc_start_time,
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

/// Reads `/proc/<pid>/stat` fields 5, 6 and 22.
pub(super) fn process_stat(pid: u32) -> Option<ProcessStat> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    Some(ProcessStat {
        pgrp: fields.get(2)?.parse().ok()?,
        session: fields.get(3)?.parse().ok()?,
        start_time: fields.get(19)?.parse().ok()?,
    })
}

/// Whether a process group still has a member in `session`.
fn group_state(pgid: u32, session: u32) -> GroupState {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return GroupState::Unknown;
    };
    let mut found = false;
    let mut unreadable = false;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        match process_stat(pid) {
            Some(stat) if stat.pgrp == pgid && (session == 0 || stat.session == session) => {
                found = true;
            }
            Some(_) => {}
            None => unreadable = true,
        }
    }
    if found {
        GroupState::Alive
    } else if unreadable {
        GroupState::Unknown
    } else {
        GroupState::Empty
    }
}

#[cfg(target_os = "linux")]
fn terminate_process_group(pid: u32) -> bool {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    killpg(Pid::from_raw(pid as i32), Signal::SIGTERM).is_ok()
}

#[cfg(not(target_os = "linux"))]
fn terminate_process_group(_pid: u32) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn kill_process_group(pid: u32) -> bool {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    killpg(Pid::from_raw(pid as i32), Signal::SIGKILL).is_ok()
}

#[cfg(not(target_os = "linux"))]
fn kill_process_group(_pid: u32) -> bool {
    false
}

impl Journal {
    /// Terminates process groups left running by a previous agent instance.
    ///
    /// The original leader must still be the recorded process, and it must be the group
    /// leader (`pgrp == pid`). After the grace period the group is identified by its PGID
    /// and session even if the leader has exited, so a surviving descendant is still
    /// signalled. An unreadable or changed identity leaves the outcome unknown instead of
    /// killing an unrelated group or claiming a clean recovery.
    pub async fn recover_orphans(&self, clock: &dyn Clock) {
        let unresolved = match self.lock() {
            Ok(state) if state.fenced.is_none() => started_without_finished(&state.records),
            _ => return,
        };
        for execution in unresolved {
            let Some(initial) = process_stat(execution.pid) else {
                // The original process is gone; the status is already outcome-unknown.
                continue;
            };
            if initial.start_time != execution.start_time || initial.pgrp != execution.pid {
                // A reused PID, or a process that was not the group leader.
                continue;
            }
            let session = if execution.session == 0 {
                initial.session
            } else {
                execution.session
            };
            if !terminate_process_group(execution.pid) {
                tracing::warn!(pid = execution.pid, "could not send SIGTERM to an orphan");
                continue;
            }
            let now = clock.now();
            let deadline = now
                .saturating_add(SignedDuration::from_secs(2))
                .unwrap_or(now);
            clock.sleep_until(deadline).await;

            let leader = process_stat(execution.pid).filter(|stat| {
                stat.start_time == execution.start_time && stat.pgrp == execution.pid
            });
            let state = match leader {
                Some(_) => GroupState::Alive,
                None => group_state(execution.pid, session),
            };
            match state {
                GroupState::Alive => {
                    if !kill_process_group(execution.pid) {
                        tracing::warn!(pid = execution.pid, "could not send SIGKILL to an orphan");
                        continue;
                    }
                }
                GroupState::Empty => {}
                GroupState::Unknown => {
                    tracing::warn!(
                        pid = execution.pid,
                        "the orphaned process group's identity is unknown; leaving it unresolved"
                    );
                    continue;
                }
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
