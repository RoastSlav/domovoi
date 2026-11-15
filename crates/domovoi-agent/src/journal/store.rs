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

/// A one-shot append failure a test injects. Never set in production.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AppendFault {
    /// Open the append file but write nothing.
    #[default]
    None,
    /// Fail before opening the file.
    Open,
    /// Write only part of the frame.
    Partial,
    /// Write the complete frame but fail before syncing it.
    Sync,
    /// Write and sync the frame, then report an uncertain outcome.
    Uncertain,
}

/// The in-memory journal, guarded by one mutex for the owning process.
pub(super) struct JournalState {
    pub(super) dir: PathBuf,
    pub(super) meta: Metadata,
    pub(super) fenced: Option<String>,
    pub(super) records: Vec<Record>,
    pub(super) valid_len: u64,
    pub(super) running_now: BTreeSet<Uuid>,
    pub(super) meta_faults: DurableFaults,
    pub(super) append_fault: AppendFault,
    pub(super) rewrite_dir_sync_fault: bool,
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
    let fault = std::mem::take(&mut state.append_fault);
    let frame = record::frame(record)?;
    let path = state.dir.join(JOURNAL_FILE);
    match write_frame(&path, &frame, fault) {
        Ok(()) => {
            state.valid_len += frame.len() as u64;
            state.records.push(record.clone());
            Ok(())
        }
        Err(err) => {
            // The file may now hold a partial frame, a complete but unsynced frame, or
            // nothing. Reconcile to whatever is actually there before returning, so the
            // in-memory list never runs ahead of the file.
            reconcile_locked(state);
            Err(JournalError::State(err))
        }
    }
}

fn write_frame(path: &Path, frame: &[u8], fault: AppendFault) -> Result<(), StateError> {
    if fault == AppendFault::Open {
        return Err(StateError::new(path, "injected append open failure"));
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|err| StateError::new(path, err))?;
    match fault {
        AppendFault::Partial => {
            let half = frame.len() / 2;
            file.write_all(&frame[..half])
                .map_err(|err| StateError::new(path, err))?;
            Err(StateError::new(path, "injected partial append failure"))
        }
        AppendFault::Sync => {
            file.write_all(frame)
                .map_err(|err| StateError::new(path, err))?;
            Err(StateError::new(path, "injected append sync failure"))
        }
        AppendFault::Uncertain => {
            file.write_all(frame)
                .and_then(|()| file.sync_all())
                .map_err(|err| StateError::new(path, err))?;
            Err(StateError::new(path, "injected uncertain append outcome"))
        }
        AppendFault::Open | AppendFault::None => file
            .write_all(frame)
            .and_then(|()| file.sync_all())
            .map_err(|err| StateError::new(path, err)),
    }
}

/// Replaces the in-memory records with what the file actually holds.
///
/// A physically incomplete tail is truncated to the last complete frame. A complete frame
/// with a bad checksum or invalid ordering fences the journal; it is never truncated.
fn reconcile_locked(state: &mut JournalState) {
    let path = state.dir.join(JOURNAL_FILE);
    let bytes = match crate::fs_atomic::read_optional(&path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            state.fenced = Some("the journal file is missing".to_owned());
            return;
        }
        Err(err) => {
            state.fenced = Some(err.to_string());
            return;
        }
    };
    match record::parse(&bytes, state.meta.epoch) {
        Ok(parsed) => {
            if parsed.valid_len < bytes.len() as u64 {
                let truncated = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|file| {
                        file.set_len(parsed.valid_len)
                            .and_then(|()| file.sync_all())
                    });
                if let Err(err) = truncated {
                    state.fenced = Some(StateError::new(&path, err).to_string());
                    return;
                }
            }
            state.records = parsed.records;
            state.valid_len = parsed.valid_len;
            state.fenced = None;
        }
        Err(err) => state.fenced = Some(err.to_string()),
    }
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
            // Indeterminate durability: never remove records and never claim either version
            // is authoritative without reading it. The visible metadata must parse, name the
            // same epoch and describe a journal that itself validates before the fence clears.
            state.fenced = Some(err.to_string());
            match Metadata::load(&state.dir) {
                Ok(Some(meta)) if meta.epoch == state.meta.epoch => {
                    match load_records(&state.dir, &meta) {
                        Ok((records, valid_len, fence)) => {
                            state.meta = meta;
                            state.records = records;
                            state.valid_len = valid_len;
                            state.fenced = fence;
                            Ok(false)
                        }
                        Err(load_err) => {
                            state.fenced = Some(load_err.to_string());
                            Ok(false)
                        }
                    }
                }
                Ok(Some(_)) => {
                    state.fenced = Some("metadata epoch changed during the write".to_owned());
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

/// The lifecycle phase of one execution in the retained records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Accepted,
    Started,
    Finished,
    Abandoned,
}

pub(super) fn phase_locked(state: &JournalState, execution_id: Uuid) -> Option<Phase> {
    let mut phase = None;
    for record in &state.records {
        match record {
            Record::Accepted {
                execution_id: id, ..
            } if *id == execution_id => phase = Some(Phase::Accepted),
            Record::Started {
                execution_id: id, ..
            } if *id == execution_id => phase = Some(Phase::Started),
            Record::Finished {
                execution_id: id, ..
            } if *id == execution_id => phase = Some(Phase::Finished),
            Record::Abandoned {
                execution_id: id, ..
            } if *id == execution_id => phase = Some(Phase::Abandoned),
            _ => {}
        }
    }
    phase
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
