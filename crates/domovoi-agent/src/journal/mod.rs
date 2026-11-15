//! The durable agent action journal: accepted, started, finished, abandoned.
//!
//! The signal spool is separate: this journal records execution lifecycle so the hub can
//! always tell "never reached the agent", "finished", "running here", or "unknowable".

mod limiter;
mod lock;
mod meta;
mod record;
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
use lock::JournalLock;
use store::{
    JournalState, absent_locked, acceptances_locked, answer_locked, append_locked, create_epoch,
    current_start_time, journal_corrupt, kept_executions, kill_process_group, load_records,
    outcome_unknown, reset_locked, rewrite_locked, started_without_finished, write_meta_locked,
};

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

/// The journal owner.
pub struct Journal {
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
        let lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        Self::open_with_lock(dir, lock, boot_id, mono_ns, limits)
    }

    /// Opens without the OS lock. Test-only; production always uses [`Journal::open`].
    #[doc(hidden)]
    pub fn open_unlocked(
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
        let Some(mut meta) = Metadata::load(dir)? else {
            return Err(JournalError::State(StateError::new(
                dir.join(META_FILE),
                "no action journal; run `domovoi agent journal init` first",
            )));
        };
        let (records, valid_len, mut fenced) = load_records(dir, &meta)?;
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
                    fenced = Some(err.to_string());
                    match Metadata::load(dir) {
                        Ok(Some(loaded)) if loaded.epoch == meta.epoch => meta = loaded,
                        Ok(_) => {}
                        Err(load_err) => fenced = Some(load_err.to_string()),
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
        };
        lock.hold();
        let journal = Self {
            state: Mutex::new(state),
            limits,
            _lock: lock,
        };
        journal.recover_orphans();
        Ok(journal)
    }

    /// Creates the journal for the first time, refusing if either journal file exists.
    pub fn init(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        let lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        Self::init_with_lock(dir, lock, now, boot_id, mono_ns)
    }

    /// Creates the journal without the OS lock. Test-only; production uses [`Journal::init`].
    #[doc(hidden)]
    pub fn init_unlocked(
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
        if dir.join(JOURNAL_FILE).exists() || dir.join(META_FILE).exists() {
            return Err(JournalError::AlreadyExists);
        }
        create_epoch(dir, now, boot_id, mono_ns)
    }

    /// Archives the existing pair and starts a new epoch, without the OS lock. Test-only.
    #[doc(hidden)]
    pub fn reset_unlocked(
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

    /// The journal's epoch.
    pub fn epoch(&self) -> Uuid {
        self.state
            .lock()
            .map(|s| s.meta.epoch)
            .unwrap_or(Uuid::nil())
    }

    /// Why the journal is fenced, when it is.
    pub fn corruption(&self) -> Option<String> {
        self.state.lock().ok().and_then(|s| s.fenced.clone())
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
        if let Some(reason) = &state.fenced {
            return Ok(AcceptOutcome::Refused(Box::new(journal_corrupt(reason))));
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
        let record = Record::Started {
            format: Record::FORMAT,
            execution_id,
            pid,
            proc_start_time,
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
        let record = Record::Finished {
            format: Record::FORMAT,
            execution_id,
            result,
        };
        append_locked(&mut state, &record)?;
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

    /// Prunes resolved history, first making the sticky marker durable.
    ///
    /// Returns whether anything was removed.
    pub fn prune(&self, keep: usize) -> Result<bool, JournalError> {
        let mut state = self.lock()?;
        if let Some(reason) = &state.fenced {
            return Err(JournalError::Corrupt(reason.clone()));
        }
        let mut committed = state.meta.pruned;
        if !state.meta.pruned {
            let mut next = state.meta.clone();
            next.pruned = true;
            committed = write_meta_locked(&mut state, &next)?;
        }
        if !committed {
            return Ok(false);
        }
        let keep_set = kept_executions(&state.records, keep);
        let mut kept = vec![state.records[0].clone()];
        for record in &state.records[1..] {
            if record
                .execution_id()
                .is_some_and(|id| keep_set.contains(&id))
            {
                kept.push(record.clone());
            }
        }
        if kept.len() == state.records.len() {
            return Ok(false);
        }
        rewrite_locked(&mut state, &kept)?;
        Ok(true)
    }

    /// Marks a corrupt journal, refusing later mutations.
    #[doc(hidden)]
    pub fn fence(&self, reason: impl Into<String>) {
        if let Ok(mut state) = self.state.lock() {
            state.fenced = Some(reason.into());
        }
    }

    /// Injects the next metadata write failures. Test-only.
    #[doc(hidden)]
    pub fn inject_metadata_faults(&self, faults: DurableFaults) {
        if let Ok(mut state) = self.state.lock() {
            state.meta_faults = faults;
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, JournalState>, JournalError> {
        self.state
            .lock()
            .map_err(|_| JournalError::Corrupt("the journal mutex was poisoned".to_owned()))
    }

    fn recover_orphans(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        if state.fenced.is_some() {
            return;
        }
        let unresolved: Vec<(Uuid, u32, u64)> = started_without_finished(&state.records);
        for (execution_id, pid, proc_start_time) in unresolved {
            if current_start_time(pid) != Some(proc_start_time) {
                continue;
            }
            kill_process_group(pid);
            let record = Record::Abandoned {
                format: Record::FORMAT,
                execution_id,
                pid,
                reason: "the agent restarted while the process was alive".to_owned(),
            };
            if let Err(err) = append_locked(&mut state, &record) {
                tracing::error!(%err, "could not record an abandoned execution");
            }
        }
    }
}
