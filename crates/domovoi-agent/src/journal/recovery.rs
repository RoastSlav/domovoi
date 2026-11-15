//! Orphan recovery after a restart: identify, terminate, and record abandoned children.

use domovoi_core::ports::Clock;
use jiff::SignedDuration;
use uuid::Uuid;

use super::Journal;
use super::record::Record;
use super::store::append_locked;

pub(super) fn started_without_finished(records: &[Record]) -> Vec<(Uuid, u32, u64)> {
    let mut started = Vec::new();
    let mut resolved = std::collections::BTreeSet::new();
    for record in records {
        match record {
            Record::Started {
                execution_id,
                pid,
                proc_start_time,
                ..
            } => started.push((*execution_id, *pid, *proc_start_time)),
            Record::Finished { execution_id, .. } | Record::Abandoned { execution_id, .. } => {
                resolved.insert(*execution_id);
            }
            _ => {}
        }
    }
    started
        .into_iter()
        .filter(|(id, _, _)| !resolved.contains(id))
        .collect()
}

#[cfg(target_os = "linux")]
pub(super) fn current_start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    fields.get(19)?.parse().ok()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn current_start_time(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
pub(super) fn terminate_process_group(pid: u32) -> bool {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    killpg(Pid::from_raw(pid as i32), Signal::SIGTERM).is_ok()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn terminate_process_group(_pid: u32) -> bool {
    false
}

#[cfg(target_os = "linux")]
pub(super) fn kill_process_group(pid: u32) -> bool {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    killpg(Pid::from_raw(pid as i32), Signal::SIGKILL).is_ok()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn kill_process_group(_pid: u32) -> bool {
    false
}

impl Journal {
    /// Terminates process groups left running by a previous agent instance.
    ///
    /// Runs after the lock and validation, never while the state mutex is held across the
    /// grace period. A PID whose start time no longer matches is never signalled.
    pub async fn recover_orphans(&self, clock: &dyn Clock) {
        let unresolved = match self.lock() {
            Ok(state) if state.fenced.is_none() => started_without_finished(&state.records),
            _ => return,
        };
        for (execution_id, pid, proc_start_time) in unresolved {
            if current_start_time(pid) != Some(proc_start_time) {
                // Dead, or a reused PID that belongs to something else.
                continue;
            }
            if !terminate_process_group(pid) {
                tracing::warn!(pid, "could not send SIGTERM to an orphaned execution");
                continue;
            }
            let now = clock.now();
            let deadline = now
                .saturating_add(SignedDuration::from_secs(2))
                .unwrap_or(now);
            clock.sleep_until(deadline).await;
            // Only signal again if the original process is still the one at this PID.
            if current_start_time(pid) == Some(proc_start_time) && !kill_process_group(pid) {
                tracing::warn!(pid, "could not send SIGKILL to an orphaned execution");
                continue;
            }
            let record = Record::Abandoned {
                format: Record::FORMAT,
                execution_id,
                pid,
                reason: "the agent restarted while the process was alive".to_owned(),
            };
            match self.lock() {
                Ok(mut state) => {
                    if let Err(err) = append_locked(&mut state, &record) {
                        tracing::error!(%err, "could not record an abandoned execution");
                    }
                }
                Err(err) => tracing::error!(%err, "could not record an abandoned execution"),
            }
        }
    }
}
