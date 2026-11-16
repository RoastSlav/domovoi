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
    /// Field 3: the one-letter process state; `Z` means a zombie, which no longer runs.
    pub(super) state: char,
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
    pgid: u32,
    session: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupState {
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

/// Reads `/proc/<pid>/stat` fields 5, 6 and 22.
pub(super) fn process_stat(pid: u32) -> Option<ProcessStat> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    Some(ProcessStat {
        state: fields.first()?.chars().next()?,
        pgrp: fields.get(2)?.parse().ok()?,
        session: fields.get(3)?.parse().ok()?,
        start_time: fields.get(19)?.parse().ok()?,
    })
}

/// Whether a process group still has a live (non-zombie) member in `session`.
pub(super) fn group_state(pgid: u32, session: u32) -> GroupState {
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
            Some(stat)
                if stat.state != 'Z'
                    && stat.pgrp == pgid
                    && (session == 0 || stat.session == session) =>
            {
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

/// Polls until the group is empty, bounded by the fake/real clock.
async fn wait_empty(clock: &dyn Clock, pgid: u32, session: u32) -> bool {
    for _ in 0..500 {
        match group_state(pgid, session) {
            GroupState::Empty => return true,
            GroupState::Unknown => return false,
            GroupState::Alive => {}
        }
        let now = clock.now();
        let next = now
            .saturating_add(SignedDuration::from_millis(100))
            .unwrap_or(now);
        clock.sleep_until(next).await;
        // With a fake clock the sleep returns at once; let the signal be delivered.
        tokio::task::yield_now().await;
    }
    matches!(group_state(pgid, session), GroupState::Empty)
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
            let initial = process_stat(execution.pid);
            let mut already_empty = false;
            let (pgid, session) = match initial {
                Some(stat) if stat.start_time == execution.start_time => {
                    // The runner starts every child as its own process-group leader, so the
                    // PID is the group id. The start time proves this is the original child.
                    (
                        execution.pid,
                        if execution.session == 0 {
                            stat.session
                        } else {
                            execution.session
                        },
                    )
                }
                Some(_) => {
                    // A reused PID, or a process whose recorded identity no longer
                    // matches. Never fall back to a group scan for this case.
                    continue;
                }
                None => {
                    // The leader has already exited. Only a persisted group leader
                    // identity can authorize a signal; a legacy record without one stays
                    // unresolved, as does a PID that was never a group leader.
                    if execution.pgid == 0
                        || execution.pgid != execution.pid
                        || execution.session == 0
                    {
                        continue;
                    }
                    match group_state(execution.pgid, execution.session) {
                        GroupState::Empty => {
                            already_empty = true;
                            (execution.pgid, execution.session)
                        }
                        GroupState::Alive => (execution.pgid, execution.session),
                        GroupState::Unknown => {
                            tracing::warn!(
                                pid = execution.pid,
                                "the orphaned process group's identity is unknown; leaving it unresolved"
                            );
                            continue;
                        }
                    }
                }
            };
            if !already_empty {
                if !terminate_process_group(pgid) {
                    tracing::warn!(pid = execution.pid, "could not send SIGTERM to an orphan");
                    continue;
                }
                let now = clock.now();
                let deadline = now
                    .saturating_add(SignedDuration::from_secs(2))
                    .unwrap_or(now);
                clock.sleep_until(deadline).await;

                match group_state(pgid, session) {
                    GroupState::Empty => {}
                    GroupState::Alive => {
                        if !kill_process_group(pgid) {
                            tracing::warn!(
                                pid = execution.pid,
                                "could not send SIGKILL to an orphan"
                            );
                            continue;
                        }
                        if !wait_empty(clock, pgid, session).await {
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
