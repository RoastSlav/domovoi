//! The durable agent action journal: accepted, started, finished, abandoned.
//!
//! The signal spool is separate: this journal records execution lifecycle so the hub can
//! always tell "never reached the agent", "finished", "running here", or "unknowable".

mod append;
mod archive;
mod commit;
mod compact;
mod compaction;
mod faults;
mod limiter;
mod lock;
mod meta;
mod record;
mod recovery;
mod reset;
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
    Fence, JournalState, Phase, absent_locked, acceptances_locked, answer_locked, journal_corrupt,
    load_records, outcome_unknown, phase_locked,
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
        Self::open_with_lock(
            dir,
            lock,
            boot_id,
            mono_ns,
            limits,
            DurableFaults::default(),
            commit::ReconcileFaults::default(),
        )
    }

    /// Opens without the OS lock. Test-only; production always uses [`Journal::open`].
    #[cfg(test)]
    pub(crate) fn open_unlocked(
        dir: &Path,
        boot_id: &str,
        mono_ns: u64,
        limits: MutationLimits,
    ) -> Result<Self, JournalError> {
        Self::open_with_lock(
            dir,
            lock::unlocked(),
            boot_id,
            mono_ns,
            limits,
            DurableFaults::default(),
            commit::ReconcileFaults::default(),
        )
    }

    /// Opens without the OS lock and injects the boot metadata write's failures. Test-only.
    #[cfg(test)]
    pub(crate) fn open_unlocked_with_boot_faults(
        dir: &Path,
        boot_id: &str,
        mono_ns: u64,
        limits: MutationLimits,
        faults: DurableFaults,
    ) -> Result<Self, JournalError> {
        Self::open_with_lock(
            dir,
            lock::unlocked(),
            boot_id,
            mono_ns,
            limits,
            faults,
            commit::ReconcileFaults::default(),
        )
    }

    /// Opens without the OS lock and injects boundary-confirmation failures. Test-only.
    #[cfg(test)]
    pub(crate) fn open_unlocked_with_reconcile_faults(
        dir: &Path,
        boot_id: &str,
        mono_ns: u64,
        limits: MutationLimits,
        faults: commit::ReconcileFaults,
    ) -> Result<Self, JournalError> {
        Self::open_with_lock(
            dir,
            lock::unlocked(),
            boot_id,
            mono_ns,
            limits,
            DurableFaults::default(),
            faults,
        )
    }

    fn open_with_lock(
        dir: &Path,
        lock: JournalLock,
        boot_id: &str,
        mono_ns: u64,
        limits: MutationLimits,
        boot_faults: DurableFaults,
        boundary_faults: commit::ReconcileFaults,
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
        // Without the durable committed length there is no statement about which bytes were
        // acknowledged, so the journal cannot be trusted even if it parses.
        let committed = commit::load(dir)?;
        // A compaction intent is resolved before the ordinary boundary rules. A shorter
        // journal is accepted only when the transaction proves it is the intended result.
        let committed = compaction::recover(dir, &meta, committed)?;
        // Never expose records from a boundary whose file and directory entry are not
        // durably confirmed. A visible value alone is not proof.
        commit::reconcile_with(dir, committed, boundary_faults)?;
        let (mut records, mut valid_len, mut fenced) = load_records(dir, &meta, committed)?;
        // A boot load is written once per boot, carrying the sticky marker forward.
        if meta.boot.boot_id != boot_id {
            let next = Metadata {
                boot: BootState {
                    boot_id: boot_id.to_owned(),
                    first_load_mono_ns: mono_ns,
                },
                ..meta.clone()
            };
            match next.write(dir, boot_faults) {
                Ok(()) => meta = next,
                Err(DurableWriteError::BeforeCommit(err)) => {
                    return Err(JournalError::State(err));
                }
                Err(DurableWriteError::Indeterminate(_err)) => {
                    // A rename may or may not be durable. Confirm the file and its directory
                    // entry, require the exact intended metadata and a journal that validates
                    // against it. A parseable journal alone is not proof.
                    match store::reconcile_metadata(dir, &next) {
                        Ok(reconciled) => {
                            meta = reconciled.meta;
                            records = reconciled.records;
                            valid_len = reconciled.valid_len;
                            fenced = reconciled.fence;
                        }
                        Err(sync_err) => {
                            return Err(JournalError::Corrupt(format!(
                                "the boot metadata's durability is unknown: {sync_err}"
                            )));
                        }
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
            commit_faults: DurableFaults::default(),
            confirm_fails: false,
            recovery_sync_fails: false,
            truncate_fails: false,
            truncate_sync_fails: false,
            rewrite_faults: compact::RewriteFaults::default(),
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

    /// Serialises every journal operation inside the owning process.
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, JournalState>, JournalError> {
        self.state
            .lock()
            .map_err(|_| JournalError::Corrupt("the journal mutex was poisoned".to_owned()))
    }
}
