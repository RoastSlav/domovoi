//! The journal's file operations: framing, records, metadata and recovery helpers.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use domovoi_proto::Open;
use domovoi_proto::messages::{ActionState, ActionStatusAnswer, OutcomeUnknownReason, Refused};
use jiff::Timestamp;
use uuid::Uuid;

use super::JOURNAL_FILE;
use super::limiter::Acceptance;
use super::meta::{META_FILE, Metadata};
use super::record::{self, Record};
use crate::error::{JournalError, StateError};
use crate::fs_atomic::{DurableFaults, DurableWriteError};

/// The in-memory journal, guarded by one mutex for the owning process.
pub(super) struct JournalState {
    pub(super) dir: PathBuf,
    pub(super) meta: Metadata,
    pub(super) fenced: Option<String>,
    pub(super) records: Vec<Record>,
    pub(super) valid_len: u64,
    pub(super) running_now: BTreeSet<Uuid>,
    pub(super) meta_faults: DurableFaults,
}

pub(super) fn load_records(
    dir: &Path,
    meta: &Metadata,
) -> Result<(Vec<Record>, u64, Option<String>), JournalError> {
    let path = dir.join(JOURNAL_FILE);
    let Some(bytes) = crate::fs_atomic::read_optional(&path)? else {
        return Ok((
            Vec::new(),
            0,
            Some("the journal file is missing".to_owned()),
        ));
    };
    if bytes.is_empty() {
        return Ok((Vec::new(), 0, Some("the journal file is empty".to_owned())));
    }
    match record::parse(&bytes, meta.epoch) {
        Ok(parsed) => {
            if parsed.valid_len < bytes.len() as u64 {
                // Only a physically incomplete final frame reaches here: truncate it so the
                // next append starts on a frame boundary.
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .map_err(|err| StateError::new(&path, err))?;
                file.set_len(parsed.valid_len)
                    .and_then(|()| file.sync_all())
                    .map_err(|err| StateError::new(&path, err))?;
            }
            Ok((parsed.records, parsed.valid_len, None))
        }
        Err(err) => Ok((Vec::new(), 0, Some(err.to_string()))),
    }
}

pub(super) fn reset_locked(
    dir: &Path,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
) -> Result<Uuid, JournalError> {
    let old = Metadata::load(dir).ok().flatten().map(|m| m.epoch);
    let suffix = old
        .map(|epoch| epoch.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    for name in [JOURNAL_FILE, META_FILE] {
        let from = dir.join(name);
        if from.exists() {
            let to = dir.join(format!("{name}.{suffix}"));
            std::fs::rename(&from, &to).map_err(|err| StateError::new(&from, err))?;
        }
    }
    create_epoch(dir, now, boot_id, mono_ns)
}

pub(super) fn create_epoch(
    dir: &Path,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
) -> Result<Uuid, JournalError> {
    std::fs::create_dir_all(dir).map_err(|err| StateError::new(dir, err))?;
    let epoch = Uuid::now_v7();
    let created = Record::JournalCreated {
        format: Record::FORMAT,
        epoch,
        wall_time: now,
        boot_id: boot_id.to_owned(),
        mono_ns,
    };
    let frame = record::frame(&created)?;
    let path = dir.join(JOURNAL_FILE);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|err| StateError::new(&path, err))?;
    file.write_all(&frame)
        .and_then(|()| file.sync_all())
        .map_err(|err| StateError::new(&path, err))?;
    drop(file);
    let meta = Metadata::new(epoch, boot_id, mono_ns);
    meta.write(dir, DurableFaults::default())
        .map_err(|err| StateError::new(dir.join(META_FILE), err))?;
    Ok(epoch)
}

pub(super) fn append_locked(state: &mut JournalState, record: &Record) -> Result<(), JournalError> {
    let frame = record::frame(record)?;
    let path = state.dir.join(JOURNAL_FILE);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|err| StateError::new(&path, err))?;
    file.write_all(&frame)
        .and_then(|()| file.sync_all())
        .map_err(|err| StateError::new(&path, err))?;
    state.valid_len += frame.len() as u64;
    state.records.push(record.clone());
    Ok(())
}

pub(super) fn rewrite_locked(
    state: &mut JournalState,
    records: &[Record],
) -> Result<(), JournalError> {
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(&record::frame(record)?);
    }
    let path = state.dir.join(JOURNAL_FILE);
    let tmp = state.dir.join(format!("{JOURNAL_FILE}.tmp"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(|err| StateError::new(&tmp, err))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|err| StateError::new(&tmp, err))?;
    drop(file);
    std::fs::rename(&tmp, &path).map_err(|err| StateError::new(&path, err))?;
    state.records = records.to_vec();
    state.valid_len = bytes.len() as u64;
    Ok(())
}

/// Writes `next` metadata, handling the three outcomes. Returns whether it committed.
pub(super) fn write_meta_locked(
    state: &mut JournalState,
    next: &Metadata,
) -> Result<bool, JournalError> {
    let faults = std::mem::take(&mut state.meta_faults);
    match next.write(&state.dir, faults) {
        Ok(()) => {
            state.meta = next.clone();
            Ok(true)
        }
        Err(DurableWriteError::BeforeCommit(err)) => Err(JournalError::State(err)),
        Err(DurableWriteError::Indeterminate(err)) => {
            // Indeterminate durability: never remove records, fence, reload whichever file is
            // visible and adopt it if it validates.
            state.fenced = Some(err.to_string());
            match Metadata::load(&state.dir) {
                Ok(Some(meta)) => {
                    state.meta = meta;
                    state.fenced = None;
                    Ok(false)
                }
                Ok(None) => Ok(false),
                Err(load_err) => {
                    state.fenced = Some(load_err.to_string());
                    Ok(false)
                }
            }
        }
    }
}

pub(super) fn acceptances_locked(state: &JournalState) -> Vec<Acceptance> {
    state
        .records
        .iter()
        .filter_map(|record| match record {
            Record::Accepted {
                execution_id,
                contract,
                boot_id,
                mono_ns,
                mutating: true,
                ..
            } => Some(Acceptance {
                execution_id: *execution_id,
                contract: contract.clone(),
                boot_id: boot_id.clone(),
                mono_ns: *mono_ns,
            }),
            _ => None,
        })
        .collect()
}

pub(super) fn answer_locked(
    state: &JournalState,
    execution_id: Uuid,
) -> Option<ActionStatusAnswer> {
    let mut accepted = false;
    let mut started = false;
    let mut finished = None;
    for record in &state.records {
        match record {
            Record::Accepted {
                execution_id: id, ..
            } if *id == execution_id => accepted = true,
            Record::Started {
                execution_id: id, ..
            } if *id == execution_id => started = true,
            Record::Finished {
                execution_id: id,
                result,
                ..
            } if *id == execution_id => finished = Some(result.clone()),
            _ => {}
        }
    }
    let state = if let Some(result) = finished {
        ActionState::Finished { result }
    } else if started {
        if state.running_now.contains(&execution_id) {
            ActionState::Running
        } else {
            ActionState::OutcomeUnknown {
                reason: Open::new(
                    "agent_restarted",
                    Some(OutcomeUnknownReason::AgentRestarted),
                ),
            }
        }
    } else if accepted {
        ActionState::OutcomeUnknown {
            reason: Open::new("no_start_record", Some(OutcomeUnknownReason::NoStartRecord)),
        }
    } else {
        return None;
    };
    Some(ActionStatusAnswer {
        execution_id,
        state,
    })
}

pub(super) fn absent_locked(state: &JournalState, execution_id: Uuid) -> ActionStatusAnswer {
    if state.fenced.is_some() || state.meta.pruned {
        outcome_unknown(execution_id, OutcomeUnknownReason::JournalLost)
    } else {
        ActionStatusAnswer {
            execution_id,
            state: ActionState::NotFound,
        }
    }
}

pub(super) fn outcome_unknown(
    execution_id: Uuid,
    reason: OutcomeUnknownReason,
) -> ActionStatusAnswer {
    let raw = match reason {
        OutcomeUnknownReason::NoStartRecord => "no_start_record",
        OutcomeUnknownReason::AgentRestarted => "agent_restarted",
        OutcomeUnknownReason::JournalLost => "journal_lost",
    };
    ActionStatusAnswer {
        execution_id,
        state: ActionState::OutcomeUnknown {
            reason: Open::new(raw, Some(reason)),
        },
    }
}

pub(super) fn journal_corrupt(reason: &str) -> Refused {
    use domovoi_proto::messages::RefusalCode;
    Refused {
        code: Open::new("journal_corrupt", Some(RefusalCode::JournalCorrupt)),
        detail: format!("the action journal is corrupt: {reason}"),
        retry_after: None,
        scope: None,
    }
}

pub(super) fn started_without_finished(records: &[Record]) -> Vec<(Uuid, u32, u64)> {
    let mut started = Vec::new();
    let mut resolved = BTreeSet::new();
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

pub(super) fn kept_executions(records: &[Record], keep: usize) -> BTreeSet<Uuid> {
    let mut accepted = BTreeSet::new();
    let mut resolved = BTreeSet::new();
    let mut finished: Vec<Uuid> = Vec::new();
    for record in records {
        match record {
            Record::Accepted { execution_id, .. } => {
                accepted.insert(*execution_id);
            }
            Record::Finished { execution_id, .. } => {
                resolved.insert(*execution_id);
                if !finished.contains(execution_id) {
                    finished.push(*execution_id);
                }
            }
            Record::Abandoned { execution_id, .. } => {
                resolved.insert(*execution_id);
            }
            _ => {}
        }
    }
    // Every unresolved acceptance is kept; so are the newest `keep` resolved ones.
    let mut kept: BTreeSet<Uuid> = accepted.difference(&resolved).copied().collect();
    for id in finished.into_iter().rev().take(keep) {
        kept.insert(id);
    }
    kept
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
pub(super) fn kill_process_group(pid: u32) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    let group = Pid::from_raw(pid as i32);
    // Signal the whole group to stop, then stop it outright. A grace period would need a
    // clock the journal does not own, and the shipped unit's cgroup already reaps leftovers.
    let _ = killpg(group, Signal::SIGTERM);
    let _ = killpg(group, Signal::SIGKILL);
}

#[cfg(not(target_os = "linux"))]
pub(super) fn kill_process_group(_pid: u32) {}
