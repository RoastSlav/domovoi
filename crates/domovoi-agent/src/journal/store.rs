//! The journal's file operations: framing, records, metadata and recovery helpers.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use domovoi_proto::Open;
use domovoi_proto::messages::{ActionState, ActionStatusAnswer, OutcomeUnknownReason, Refused};
use jiff::Timestamp;
use uuid::Uuid;

use super::JOURNAL_FILE;
use super::commit;
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
    /// Write the complete frame, fail both syncs, repair the length but fail its sync.
    RecoverySyncTruncateSyncFail,
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
    pub(super) commit_faults: DurableFaults,
    pub(super) confirm_fails: bool,
    pub(super) recovery_sync_fails: bool,
    pub(super) truncate_fails: bool,
    pub(super) truncate_sync_fails: bool,
    pub(super) rewrite_faults: super::compact::RewriteFaults,
}

pub(super) fn load_records(
    dir: &Path,
    meta: &Metadata,
    committed: u64,
) -> Result<(Vec<Record>, u64, Option<Fence>), JournalError> {
    let path = dir.join(JOURNAL_FILE);
    let Some(mut bytes) = crate::fs_atomic::read_optional(&path)? else {
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
    let actual = bytes.len() as u64;
    if actual < committed {
        // Only a durable compaction transaction may explain a shorter journal. Without one,
        // a complete, lifecycle-valid prefix is not proof that the missing records were
        // legitimately removed, so never lower the boundary or adopt it.
        return Ok((
            Vec::new(),
            0,
            Some(Fence::corrupt(format!(
                "the journal is {actual} bytes, shorter than the committed {committed} bytes"
            ))),
        ));
    }
    if actual > committed {
        // Bytes beyond the boundary were never acknowledged. They may form a complete,
        // checksum-valid frame, but no commit point ever named them.
        commit::truncate_journal(dir, committed)?;
        bytes.truncate(committed as usize);
    }
    match record::parse(&bytes, meta.epoch) {
        Ok(parsed) if parsed.valid_len == committed => Ok((parsed.records, committed, None)),
        Ok(parsed) => Ok((
            Vec::new(),
            0,
            Some(Fence::corrupt(format!(
                "the journal ends at {} bytes inside the committed {committed} bytes",
                parsed.valid_len
            ))),
        )),
        Err(err) => Ok((Vec::new(), 0, Some(Fence::corrupt(err.to_string())))),
    }
}

/// Marks an in-progress explicit reset so a retry can finish it instead of restarting it.
pub(super) const RESET_INTENT_FILE: &str = "action-journal.reset";

/// Creates the journal, its commit boundary and its metadata for a known epoch.
///
/// The order is journal frame (synced), then the durable committed length, then metadata.
/// The boundary write syncs the parent directory, so the journal's directory entry is
/// durable by the time an append can be acknowledged.
pub(super) fn create_epoch_with(
    dir: &Path,
    epoch: Uuid,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
    commit_faults: DurableFaults,
) -> Result<Uuid, JournalError> {
    std::fs::create_dir_all(dir).map_err(|err| StateError::new(dir, err))?;
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
    commit::write(dir, frame.len() as u64, commit_faults)
        .map_err(|err| StateError::new(dir.join(commit::COMMIT_FILE), err))?;
    let meta = Metadata::new(epoch, boot_id, mono_ns);
    meta.write(dir, DurableFaults::default())
        .map_err(|err| StateError::new(dir.join(META_FILE), err))?;
    Ok(epoch)
}

/// The state implied by metadata that has been proven durable.
#[derive(Debug)]
pub(super) struct Reconciled {
    pub(super) meta: Metadata,
    pub(super) records: Vec<Record>,
    pub(super) valid_len: u64,
    pub(super) fence: Option<Fence>,
}

/// Proves that `expected` is the durable on-disk metadata and that the journal agrees.
///
/// This is the single reconciliation contract for startup and live metadata writes. It
/// syncs the metadata file and its directory entry, requires the visible object to equal
/// the intended one exactly, then loads the journal under the durable commit boundary. Any
/// failure is returned, and the caller must keep its fence.
pub(super) fn reconcile_metadata(
    dir: &Path,
    expected: &Metadata,
) -> Result<Reconciled, JournalError> {
    let path = dir.join(META_FILE);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| file.sync_all())
        .map_err(|err| StateError::new(&path, err))?;
    crate::fs_atomic::sync_dir(dir).map_err(|err| StateError::new(dir, err))?;
    let Some(visible) = Metadata::load(dir)? else {
        return Err(JournalError::Corrupt(
            "the metadata disappeared during its write".to_owned(),
        ));
    };
    if visible != *expected {
        return Err(JournalError::Corrupt(format!(
            "the visible metadata is not the intended value: expected epoch {} boot {} \
             first load {}, found epoch {} boot {} first load {} pruned {}",
            expected.epoch,
            expected.boot.boot_id,
            expected.boot.first_load_mono_ns,
            visible.epoch,
            visible.boot.boot_id,
            visible.boot.first_load_mono_ns,
            visible.pruned
        )));
    }
    if super::compaction::exists(dir) {
        return Err(JournalError::Corrupt(
            "a compaction transaction is unresolved; refusing to adopt metadata".to_owned(),
        ));
    }
    // Adoption of any journal state goes through the same confirmed boundary.
    let committed = commit::load(dir)?;
    commit::reconcile(dir, committed)?;
    let (records, valid_len, fence) = load_records(dir, &visible, committed)?;
    Ok(Reconciled {
        meta: visible,
        records,
        valid_len,
        fence,
    })
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
            // Indeterminate durability: fence first, then prove the intended object and its
            // durability with the same reconciliation contract startup uses. A visible file
            // with the same epoch is not proof; only exact equality after a confirmed sync is.
            state.fenced = Some(Fence::metadata(err.to_string()));
            match reconcile_metadata(&state.dir, next) {
                Ok(reconciled) => {
                    state.meta = reconciled.meta;
                    state.records = reconciled.records;
                    state.valid_len = reconciled.valid_len;
                    state.fenced = reconciled.fence;
                    Ok(false)
                }
                Err(reconcile_err) => {
                    state.fenced = Some(Fence::metadata(reconcile_err.to_string()));
                    Err(reconcile_err)
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
