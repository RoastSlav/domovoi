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
    /// Write the complete frame but fail before syncing it; a recovery sync may succeed.
    Sync,
    /// Write the complete frame, fail its sync, and fail the recovery sync too.
    RecoverySync,
    /// Write the complete frame, fail both syncs, and fail the repair truncation.
    RecoverySyncTruncateFail,
    /// Write and sync the frame, then report an uncertain outcome.
    Uncertain,
}

/// Why a journal refuses mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FenceKind {
    /// Permanent corruption; only a local reset recovers.
    Corrupt,
    /// A metadata write's durability is unknown; only a validated reload may clear it.
    Metadata,
    /// An append's durability is unknown; only a successful recovery sync may clear it.
    Append,
}

/// A fence and its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fence {
    /// What kind of condition holds.
    pub(super) kind: FenceKind,
    /// What is wrong, for the refusal detail and logs.
    pub(super) reason: String,
}

impl Fence {
    pub(super) fn corrupt(reason: impl Into<String>) -> Self {
        Self {
            kind: FenceKind::Corrupt,
            reason: reason.into(),
        }
    }

    pub(super) fn metadata(reason: impl Into<String>) -> Self {
        Self {
            kind: FenceKind::Metadata,
            reason: reason.into(),
        }
    }

    pub(super) fn append(reason: impl Into<String>) -> Self {
        Self {
            kind: FenceKind::Append,
            reason: reason.into(),
        }
    }
}

/// The in-memory journal, guarded by one mutex for the owning process.
pub(super) struct JournalState {
    pub(super) dir: PathBuf,
    pub(super) meta: Metadata,
    pub(super) fenced: Option<Fence>,
    pub(super) records: Vec<Record>,
    pub(super) valid_len: u64,
    pub(super) running_now: BTreeSet<Uuid>,
    pub(super) meta_faults: DurableFaults,
    pub(super) append_fault: AppendFault,
    pub(super) recovery_sync_fails: bool,
    pub(super) truncate_fails: bool,
    pub(super) rewrite_dir_sync_fault: bool,
}

pub(super) fn load_records(
    dir: &Path,
    meta: &Metadata,
) -> Result<(Vec<Record>, u64, Option<Fence>), JournalError> {
    let path = dir.join(JOURNAL_FILE);
    let Some(bytes) = crate::fs_atomic::read_optional(&path)? else {
        return Ok((
            Vec::new(),
            0,
            Some(Fence::corrupt("the journal file is missing")),
        ));
    };
    if bytes.is_empty() {
        return Ok((
            Vec::new(),
            0,
            Some(Fence::corrupt("the journal file is empty")),
        ));
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
        Err(err) => Ok((Vec::new(), 0, Some(Fence::corrupt(err.to_string())))),
    }
}

/// Marks an in-progress explicit reset so a retry can finish it instead of restarting it.
pub(super) const RESET_INTENT_FILE: &str = "action-journal.reset";

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
            state.fenced = Some(Fence::metadata(err.to_string()));
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
                            state.fenced = Some(Fence::metadata(load_err.to_string()));
                            Ok(false)
                        }
                    }
                }
                Ok(Some(_)) => {
                    state.fenced = Some(Fence::metadata("metadata epoch changed during the write"));
                    Ok(false)
                }
                Ok(None) => Ok(false),
                Err(load_err) => {
                    state.fenced = Some(Fence::metadata(load_err.to_string()));
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
