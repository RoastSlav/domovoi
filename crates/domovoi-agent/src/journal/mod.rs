//! The durable agent action journal: accepted, started, finished, abandoned.
//!
//! The signal spool is separate: this journal records execution lifecycle so the hub can
//! always tell "never reached the agent", "finished", "running here", or "unknowable".

mod append;
mod compact;
mod limiter;
mod lock;
mod meta;
mod record;
mod recovery;
mod store;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use domovoi_config::MutationLimits;
use domovoi_proto::messages::{
    ActionStatusAnswer, ActionStatusAnswers, ActionStatusQuery, AuthorizationKind,
    CapabilityResult, OutcomeUnknownReason, Refused,
};
use jiff::Timestamp;
use uuid::Uuid;

use crate::error::{JournalError, StateError};
use crate::fs_atomic::{DurableFaults, DurableWriteError};
use append::append_locked;
use lock::JournalLock;
use store::{
    Fence, JournalState, Phase, absent_locked, acceptances_locked, answer_locked, create_epoch,
    journal_corrupt, load_records, outcome_unknown, phase_locked, reset_locked,
};

/// A one-shot append failure a test can inject. Never set in production.
#[doc(hidden)]
pub use store::AppendFault;

#[cfg(test)]
mod tests;

pub use limiter::{Acceptance, LimitCheck};
pub use meta::{BootState, META_FILE, Metadata};
pub use record::{MAX_PAYLOAD, Record};

/// The journal's append-only record file.
pub const JOURNAL_FILE: &str = "action-journal.bin";

/// What `accept` decided.
#[derive(Debug, Clone, PartialEq)]
pub enum AcceptOutcome {
    /// The execution is durable and may proceed.
    Accepted,
    /// The execution was already known; answer from its record.
    Duplicate(ActionStatusAnswer),
    /// The execution was refused before acceptance.
    Refused(Box<Refused>),
}

/// A mutating or read-only execution asking to be accepted.
#[derive(Debug, Clone)]
pub struct AcceptedRequest<'a> {
    /// The execution.
    pub execution_id: Uuid,
    /// The contract id.
    pub contract: &'a str,
    /// The contract version.
    pub version: u32,
    /// The canonical target.
    pub target: &'a str,
    /// The plan hash, as lowercase hex.
    pub plan_hash: &'a str,
    /// Who authorized it.
    pub authorization_kind: AuthorizationKind,
    /// Whether the contract mutates the host.
    pub mutating: bool,
}

impl std::fmt::Debug for Journal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Journal")
            .field("epoch", &self.epoch)
            .field("fenced", &self.corruption().is_some())
            .finish_non_exhaustive()
    }
}

/// The journal owner.
pub struct Journal {
    /// Immutable for this owner; never read through the mutable state mutex.
    epoch: Uuid,
    state: Mutex<JournalState>,
    limits: MutationLimits,
    _lock: JournalLock,
}

impl Journal {
    /// Opens the journal under `dir`, taking the exclusive cross-process lock first.
    ///
    /// Fails when metadata is missing or invalid. A valid metadata file with a missing,
    /// empty or mismatched journal opens fenced: reads work, mutations are refused.
    pub fn open(
        dir: &Path,
        boot_id: &str,
        mono_ns: u64,
        limits: MutationLimits,
    ) -> Result<Self, JournalError> {
        // The lock file lives in the state directory; create the directory before opening
        // it, then take the lock before inspecting either journal file.
        std::fs::create_dir_all(dir).map_err(|err| StateError::new(dir, err))?;
        let lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        Self::open_with_lock(dir, lock, boot_id, mono_ns, limits)
    }

    /// Opens without the OS lock. Test-only; production always uses [`Journal::open`].
    #[cfg(test)]
    pub(crate) fn open_unlocked(
        dir: &Path,
        boot_id: &str,
        mono_ns: u64,
        limits: MutationLimits,
    ) -> Result<Self, JournalError> {
        Self::open_with_lock(dir, lock::unlocked(), boot_id, mono_ns, limits)
    }

    fn open_with_lock(
        dir: &Path,
        lock: JournalLock,
        boot_id: &str,
        mono_ns: u64,
        limits: MutationLimits,
    ) -> Result<Self, JournalError> {
        if dir.join(store::RESET_INTENT_FILE).exists() {
            return Err(JournalError::State(StateError::new(
                dir.join(store::RESET_INTENT_FILE),
                "an interrupted journal reset; run `domovoi agent journal reset`",
            )));
        }
        let mut meta = match Metadata::load(dir)? {
            Some(meta) => meta,
            None => {
                // The journal file is the only evidence of a partial initialization. Never
                // treat it as a fresh install and never create an epoch here.
                let detail = if dir.join(JOURNAL_FILE).exists() {
                    "the action journal is partially initialized; run `domovoi agent journal reset`"
                } else {
                    "no action journal; run `domovoi agent journal init` first"
                };
                return Err(JournalError::State(StateError::new(
                    dir.join(META_FILE),
                    detail,
                )));
            }
        };
        let (mut records, mut valid_len, mut fenced) = load_records(dir, &meta)?;
        // A boot load is written once per boot, carrying the sticky marker forward.
        if meta.boot.boot_id != boot_id {
            let next = Metadata {
                boot: BootState {
                    boot_id: boot_id.to_owned(),
                    first_load_mono_ns: mono_ns,
                },
                ..meta.clone()
            };
            match next.write(dir, DurableFaults::default()) {
                Ok(()) => meta = next,
                Err(DurableWriteError::BeforeCommit(err)) => {
                    return Err(JournalError::State(err));
                }
                Err(DurableWriteError::Indeterminate(err)) => {
                    fenced = Some(Fence::metadata(err.to_string()));
                    match Metadata::load(dir) {
                        Ok(Some(loaded)) if loaded.epoch == meta.epoch => {
                            match load_records(dir, &loaded) {
                                Ok((kept, len, fence)) => {
                                    meta = loaded;
                                    records = kept;
                                    valid_len = len;
                                    fenced = fence;
                                }
                                Err(load_err) => {
                                    fenced = Some(Fence::metadata(load_err.to_string()));
                                }
                            }
                        }
                        Ok(Some(_)) => {
                            fenced =
                                Some(Fence::metadata("metadata epoch changed during the write"));
                        }
                        Ok(None) => {}
                        Err(load_err) => fenced = Some(Fence::metadata(load_err.to_string())),
                    }
                }
            }
        }
        let state = JournalState {
            dir: dir.to_path_buf(),
            meta,
            fenced,
            records,
            valid_len,
            running_now: BTreeSet::new(),
            meta_faults: DurableFaults::default(),
            append_fault: AppendFault::default(),
            rewrite_dir_sync_fault: false,
        };
        lock.hold();
        let epoch = state.meta.epoch;
        Ok(Self {
            epoch,
            state: Mutex::new(state),
            limits,
            _lock: lock,
        })
    }

    /// Creates the journal for the first time, refusing if either journal file exists.
    pub fn init(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        std::fs::create_dir_all(dir).map_err(|err| StateError::new(dir, err))?;
        let lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        Self::init_with_lock(dir, lock, now, boot_id, mono_ns)
    }

    /// Creates the journal without the OS lock. Test-only; production uses [`Journal::init`].
    #[cfg(test)]
    pub(crate) fn init_unlocked(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        Self::init_with_lock(dir, lock::unlocked(), now, boot_id, mono_ns)
    }

    fn init_with_lock(
        dir: &Path,
        lock: JournalLock,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock;
        if dir.join(JOURNAL_FILE).exists()
            || dir.join(META_FILE).exists()
            || dir.join(store::RESET_INTENT_FILE).exists()
        {
            return Err(JournalError::AlreadyExists);
        }
        create_epoch(dir, now, boot_id, mono_ns)
    }

    /// Archives the existing pair and starts a new epoch, without the OS lock. Test-only.
    #[cfg(test)]
    pub(crate) fn reset_unlocked(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock::unlocked();
        reset_locked(dir, now, boot_id, mono_ns)
    }

    /// Archives the existing pair and starts a new epoch.
    pub fn reset(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        reset_locked(dir, now, boot_id, mono_ns)
    }

    /// The journal's epoch. Immutable for this owner, so a poisoned state mutex cannot
    /// change or hide it.
    pub fn epoch(&self) -> Uuid {
        self.epoch
    }

    /// Why the journal is fenced, when it is.
    pub fn corruption(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.fenced.as_ref().map(|fence| fence.reason.clone()))
    }

    /// Accepts `request`, writing a durable `accepted` record before returning.
    pub fn accept(
        &self,
        request: &AcceptedRequest<'_>,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<AcceptOutcome, JournalError> {
        let mut state = self.lock()?;
        if let Some(fence) = &state.fenced {
            return Ok(AcceptOutcome::Refused(Box::new(journal_corrupt(
                &fence.reason,
            ))));
        }
        if let Some(answer) = answer_locked(&state, request.execution_id) {
            return Ok(AcceptOutcome::Duplicate(answer));
        }
        if request.mutating {
            let accepted = acceptances_locked(&state);
            let check = limiter::check(
                &self.limits,
                &accepted,
                request.contract,
                mono_ns,
                boot_id,
                state.meta.boot.first_load_mono_ns,
            );
            if let LimitCheck::Refused(refused) = check {
                return Ok(AcceptOutcome::Refused(Box::new(refused)));
            }
        }
        let record = Record::Accepted {
            format: Record::FORMAT,
            execution_id: request.execution_id,
            contract: request.contract.to_owned(),
            version: request.version,
            target: request.target.to_owned(),
            plan_hash: request.plan_hash.to_owned(),
            authorization_kind: request.authorization_kind,
            wall_time: now,
            boot_id: boot_id.to_owned(),
            mono_ns,
            mutating: request.mutating,
        };
        append_locked(&mut state, &record)?;
        Ok(AcceptOutcome::Accepted)
    }

    /// Writes a durable `started` record and marks the execution as running here.
    pub fn started(
        &self,
        execution_id: Uuid,
        pid: u32,
        proc_start_time: u64,
    ) -> Result<(), JournalError> {
        let mut state = self.lock()?;
        if let Some(fence) = &state.fenced {
            return Err(JournalError::Corrupt(fence.reason.clone()));
        }
        if phase_locked(&state, execution_id) != Some(Phase::Accepted) {
            return Err(JournalError::Lifecycle(format!(
                "started for {execution_id} requires a preceding accepted record"
            )));
        }
        let stat = recovery::process_stat(pid);
        let record = Record::Started {
            format: Record::FORMAT,
            execution_id,
            pid,
            proc_start_time,
            pgid: stat.map_or(0, |stat| stat.pgrp),
            session: stat.map_or(0, |stat| stat.session),
        };
        append_locked(&mut state, &record)?;
        state.running_now.insert(execution_id);
        Ok(())
    }

    /// Writes a durable `finished` record before the reply is sent.
    pub fn finished(
        &self,
        execution_id: Uuid,
        result: CapabilityResult,
    ) -> Result<(), JournalError> {
        let mut state = self.lock()?;
        if let Some(fence) = &state.fenced {
            return Err(JournalError::Corrupt(fence.reason.clone()));
        }
        if phase_locked(&state, execution_id) != Some(Phase::Started) {
            return Err(JournalError::Lifecycle(format!(
                "finished for {execution_id} requires a preceding started record"
            )));
        }
        let record = Record::Finished {
            format: Record::FORMAT,
            execution_id,
            result,
        };
        if let Err(err) = append_locked(&mut state, &record) {
            // The child may already have finished, but its result is not durable. Stop
            // claiming it is running; the status falls back to outcome-unknown.
            state.running_now.remove(&execution_id);
            return Err(err);
        }
        state.running_now.remove(&execution_id);
        Ok(())
    }

    /// Answers a status query in request order.
    pub fn action_status(
        &self,
        query: &ActionStatusQuery,
    ) -> Result<ActionStatusAnswers, JournalError> {
        let state = self.lock()?;
        let answers = query
            .execution_ids
            .iter()
            .map(|id| {
                if query.expected_epoch != state.meta.epoch {
                    outcome_unknown(*id, OutcomeUnknownReason::JournalLost)
                } else {
                    answer_locked(&state, *id).unwrap_or_else(|| absent_locked(&state, *id))
                }
            })
            .collect();
        Ok(ActionStatusAnswers {
            epoch: state.meta.epoch,
            answers,
        })
    }

    /// Marks a corrupt journal, refusing later mutations.
    #[doc(hidden)]
    pub fn fence(&self, reason: impl Into<String>) {
        if let Ok(mut state) = self.state.lock() {
            state.fenced = Some(Fence::corrupt(reason));
        }
    }

    /// Injects the next metadata write failures. Test-only.
    #[doc(hidden)]
    pub fn inject_metadata_faults(&self, faults: DurableFaults) {
        if let Ok(mut state) = self.state.lock() {
            state.meta_faults = faults;
        }
    }

    /// Injects the next append failure. Test-only.
    #[doc(hidden)]
    pub fn inject_append_fault(&self, fault: AppendFault) {
        if let Ok(mut state) = self.state.lock() {
            state.append_fault = fault;
        }
    }

    /// Injects the next compaction directory-sync failure. Test-only.
    #[doc(hidden)]
    pub fn inject_rewrite_dir_sync_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.rewrite_dir_sync_fault = true;
        }
    }

    /// Poisons the state mutex. Test-only.
    #[cfg(test)]
    pub(crate) fn poison_state_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.state.lock().expect("the journal lock is healthy");
            panic!("poison the journal lock");
        }));
    }

    /// Serialises every journal operation inside the owning process.
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, JournalState>, JournalError> {
        self.state
            .lock()
            .map_err(|_| JournalError::Corrupt("the journal mutex was poisoned".to_owned()))
    }
}
