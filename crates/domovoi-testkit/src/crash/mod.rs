//! Fault injection, snapshots and invariants for the crash sweep.
//!
//! [`CrashingStore`] wraps a store and makes one chosen commit fail the way a process
//! death at that point would: before the commit is durable (nothing applied) or after it
//! is durable but before the caller sees the receipt (everything applied). The services
//! keep reading through the inner store; only their commits pass through the wrapper.
//!
//! [`Snapshot`] and [`check_invariants`] describe and verify durable state without using
//! ids, so a run interrupted at any commit can be compared with the fault-free prefix and
//! checked for the F1 cross-table invariants.

mod invariants;
#[cfg(test)]
mod seams;
mod snapshot;
#[cfg(test)]
mod tests;

#[cfg(test)]
use seams::Hold;

pub use invariants::{Exempt, check_invariants, check_invariants_except};
pub use snapshot::Snapshot;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    AckRepo, ActionGuard, ActionRepo, AuditLog, CommitError, CommitReceipt, EffectRead, Expected,
    IncidentRepo, JournalRead, SignalRepo, StateWrite, StoreError, Transaction, UnitOfWork,
    VersionedRead,
};

use crate::lock;

/// Where a simulated crash happens relative to one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    /// The process dies before the commit becomes durable: nothing is applied.
    BeforeCommit,
    /// The process dies after the commit is durable, before the caller sees the receipt.
    AfterCommit,
}

/// Which commit crashes: the `at_commit`-th non-empty commit (1-based) since arming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashPlan {
    /// The 1-based index of the commit that crashes.
    pub at_commit: u64,
    /// Where, relative to that commit, the process dies.
    pub point: CrashPoint,
}

/// What the storage under test must provide: the unit of work, its readers and the
/// repository ports the services read through.
pub trait SweepStore:
    UnitOfWork
    + VersionedRead
    + JournalRead
    + EffectRead
    + IncidentRepo
    + SignalRepo
    + ActionRepo
    + AuditLog
    + AckRepo
    + Send
    + Sync
    + 'static
{
}

impl<T> SweepStore for T where
    T: UnitOfWork
        + VersionedRead
        + JournalRead
        + EffectRead
        + IncidentRepo
        + SignalRepo
        + ActionRepo
        + AuditLog
        + AckRepo
        + Send
        + Sync
        + 'static
{
}

/// A short, id-free description of one commit, for coverage checks and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxSummary {
    /// One string per [`StateWrite`], in write order.
    pub writes: Vec<String>,
    /// One journal kind per row the commit appends, including mirrored timeline rows.
    pub journal: Vec<String>,
    /// One kind per audit row the commit appends.
    pub audit: Vec<String>,
}

/// Describes a transaction without ids or payload detail.
fn summarize(tx: &Transaction) -> TxSummary {
    let mut writes = Vec::with_capacity(tx.writes.len());
    let mut journal = Vec::with_capacity(tx.writes.len() + tx.journal.len());
    for write in &tx.writes {
        match write {
            StateWrite::Signal { .. } => writes.push("signal".to_owned()),
            StateWrite::Incident { expected, .. } => writes.push(match expected {
                Expected::New => "incident:new".to_owned(),
                Expected::Version(_) => "incident:version".to_owned(),
            }),
            StateWrite::IncidentEvent(entry) => {
                writes.push("incident_event".to_owned());
                journal.push(
                    domovoi_core::ports::timeline_journal(entry)
                        .kind
                        .as_str()
                        .to_owned(),
                );
            }
            StateWrite::DiscardPendingIncident { .. } => writes.push("incident:discard".to_owned()),
            StateWrite::Action { action, expected } => writes.push(match expected {
                ActionGuard::New => "action:new".to_owned(),
                ActionGuard::Status(_) => {
                    format!("action:status:{}", action.status().name())
                }
            }),
            StateWrite::Note { expected, .. } => writes.push(match expected {
                Expected::New => "note:new".to_owned(),
                Expected::Version(_) => "note:version".to_owned(),
            }),
        }
    }
    journal.extend(
        tx.journal
            .iter()
            .map(|draft| draft.kind.as_str().to_owned()),
    );
    TxSummary {
        writes,
        journal,
        audit: tx.audit.iter().map(|draft| draft.kind.clone()).collect(),
    }
}

/// Mutable state of the wrapper, kept behind one lock.
#[derive(Default)]
struct PlanState {
    plan: Option<CrashPlan>,
    commits: u64,
    crashed: bool,
    summaries: Vec<TxSummary>,
    recording: bool,
    snapshots: Vec<Snapshot>,
}

/// Wraps a store; the services' *commits* pass through it, reads go to [`inner`].
///
/// Only one commit is applied at a time. Without that, two commits could be inside the inner
/// store together: the crash could be armed for the first while the second still applied, so
/// the surviving state would carry a write past the crash point, and a snapshot taken after
/// commit `k` could see commit `k + 1`.
///
/// [`inner`]: CrashingStore::inner
pub struct CrashingStore<S: SweepStore> {
    inner: Arc<S>,
    state: Mutex<PlanState>,
    serial: tokio::sync::Mutex<()>,
    /// Test-only hold between a durable write and its snapshot.
    #[cfg(test)]
    hold: Mutex<Option<Arc<Hold>>>,
    /// Test-only hold after a transaction is counted and before it is applied.
    #[cfg(test)]
    apply_hold: Mutex<Option<Arc<Hold>>>,
    /// Test-only failure returned by the next snapshot.
    #[cfg(test)]
    snapshot_failure: Mutex<Option<StoreError>>,
}

impl<S: SweepStore> CrashingStore<S> {
    /// Wraps `inner` with no plan and no recording.
    #[must_use]
    pub fn new(inner: Arc<S>) -> Self {
        Self {
            inner,
            state: Mutex::new(PlanState::default()),
            serial: tokio::sync::Mutex::new(()),
            #[cfg(test)]
            hold: Mutex::new(None),
            #[cfg(test)]
            apply_hold: Mutex::new(None),
            #[cfg(test)]
            snapshot_failure: Mutex::new(None),
        }
    }

    /// The store the services read through and that survives a restart.
    #[must_use]
    pub fn inner(&self) -> Arc<S> {
        self.inner.clone()
    }

    /// Arms one crash. Replaces any earlier plan and resets the commit counter, the
    /// summaries and the crashed flag.
    pub fn arm(&self, plan: CrashPlan) {
        let mut state = lock(&self.state);
        state.plan = Some(plan);
        state.commits = 0;
        state.crashed = false;
        state.summaries.clear();
        state.recording = false;
        state.snapshots.clear();
    }

    /// Disarms and resets the counter, summaries and snapshots: a fault-free run that
    /// only records.
    pub fn record_only(&self) {
        let mut state = lock(&self.state);
        state.plan = None;
        state.commits = 0;
        state.crashed = false;
        state.summaries.clear();
        state.recording = false;
        state.snapshots.clear();
    }

    /// Number of non-empty commits attempted since arming or [`record_only`]; a crashed
    /// or refused commit counts.
    ///
    /// [`record_only`]: CrashingStore::record_only
    #[must_use]
    pub fn commits(&self) -> u64 {
        lock(&self.state).commits
    }

    /// Whether the planned crash has happened (the process is "down").
    #[must_use]
    pub fn crashed(&self) -> bool {
        lock(&self.state).crashed
    }

    /// One summary per attempted commit, in order.
    #[must_use]
    pub fn summaries(&self) -> Vec<TxSummary> {
        lock(&self.state).summaries.clone()
    }

    /// Marks the process as restarted: clears the crashed flag and the plan, resets the
    /// commit counter and keeps the recorded summaries.
    pub fn restart(&self) {
        let mut state = lock(&self.state);
        state.plan = None;
        state.commits = 0;
        state.crashed = false;
    }

    /// Starts recording a [`Snapshot`] of the inner store: one now (index 0, the state
    /// before any commit) and one after every commit the inner store applied successfully.
    pub async fn record_snapshots(&self) -> Result<(), StoreError> {
        let first = Snapshot::capture(&*self.inner).await?;
        let mut state = lock(&self.state);
        state.snapshots.clear();
        state.snapshots.push(first);
        state.recording = true;
        Ok(())
    }

    /// The recorded snapshots, index i = state after the i-th commit (0 = before the first).
    #[must_use]
    pub fn snapshots(&self) -> Vec<Snapshot> {
        lock(&self.state).snapshots.clone()
    }

    /// Captures the inner store and appends the snapshot when recording is on.
    async fn capture_after_commit(&self) -> Result<(), StoreError> {
        if !lock(&self.state).recording {
            return Ok(());
        }
        #[cfg(test)]
        let hold = lock(&self.hold).take();
        #[cfg(test)]
        if let Some(hold) = hold {
            hold.pause().await;
        }
        #[cfg(test)]
        let snapshot_failure = lock(&self.snapshot_failure).take();
        #[cfg(test)]
        if let Some(error) = snapshot_failure {
            return Err(error);
        }
        let snapshot = Snapshot::capture(&*self.inner).await?;
        let mut state = lock(&self.state);
        if state.recording {
            state.snapshots.push(snapshot);
        }
        Ok(())
    }
}

#[async_trait]
impl<S: SweepStore> UnitOfWork for CrashingStore<S> {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        if tx.is_empty() {
            return self.inner.commit(tx).await;
        }

        // One commit at a time: the next one can only start once this one is applied and,
        // if it is the planned crash, the process is already down.
        let _serial = self.serial.lock().await;

        // Decide while holding the lock, then release it before any await.
        let (nth, plan, recording) = {
            let mut state = lock(&self.state);
            if state.crashed {
                state.commits += 1;
                state.summaries.push(summarize(&tx));
                return Err(CommitError::Backend("process is down".to_owned()));
            }
            state.commits += 1;
            state.summaries.push(summarize(&tx));
            (state.commits, state.plan, state.recording)
        };

        #[cfg(test)]
        let apply_hold = lock(&self.apply_hold).take();
        #[cfg(test)]
        if let Some(hold) = apply_hold {
            hold.pause().await;
        }

        if let Some(plan) = plan
            && plan.at_commit == nth
        {
            match plan.point {
                CrashPoint::BeforeCommit => {
                    lock(&self.state).crashed = true;
                    return Err(CommitError::Backend(
                        "simulated crash before commit".to_owned(),
                    ));
                }
                CrashPoint::AfterCommit => {
                    let result = self.inner.commit(tx).await;
                    // The process is down from here on, whatever the store did, so nothing
                    // queued behind this commit may reach the store.
                    lock(&self.state).crashed = true;
                    if result.is_ok()
                        && recording
                        && let Err(err) = self.capture_after_commit().await
                    {
                        // The write is durable and the receipt is lost either way; say that
                        // the harness could not picture it rather than hide the failure.
                        return Err(CommitError::Backend(format!(
                            "the transaction is durable but its snapshot failed at the \
                             simulated crash: {err}"
                        )));
                    }
                    if let Err(err) = &result {
                        // Nothing is durable: the store failed before the process died.
                        return Err(CommitError::Backend(format!(
                            "the inner store failed, so nothing was stored before the \
                             simulated crash: {err}"
                        )));
                    }
                    return Err(CommitError::Backend(
                        "simulated crash after commit".to_owned(),
                    ));
                }
            }
        }

        let result = self.inner.commit(tx).await;
        if result.is_ok() && recording {
            self.capture_after_commit()
                .await
                .map_err(|err| CommitError::Backend(err.to_string()))?;
        }
        result
    }
}

#[async_trait]
impl<S: SweepStore> VersionedRead for CrashingStore<S> {
    async fn incident(
        &self,
        id: domovoi_core::IncidentId,
    ) -> Result<Option<domovoi_core::ports::Versioned<domovoi_core::Incident>>, StoreError> {
        self.inner.incident(id).await
    }

    async fn active_incidents(
        &self,
    ) -> Result<Vec<domovoi_core::ports::Versioned<domovoi_core::Incident>>, StoreError> {
        self.inner.active_incidents().await
    }

    async fn note(
        &self,
        id: domovoi_core::NoteId,
    ) -> Result<Option<domovoi_core::ports::Versioned<domovoi_core::Note>>, StoreError> {
        self.inner.note(id).await
    }
}

#[async_trait]
impl<S: SweepStore> JournalRead for CrashingStore<S> {
    async fn entries_after(
        &self,
        after_seq: u64,
        limit: u16,
    ) -> Result<Vec<domovoi_core::ports::JournalEntry>, StoreError> {
        self.inner.entries_after(after_seq, limit).await
    }

    async fn entries_for(
        &self,
        aggregate: &domovoi_core::ports::AggregateRef,
        limit: u16,
    ) -> Result<Vec<domovoi_core::ports::JournalEntry>, StoreError> {
        self.inner.entries_for(aggregate, limit).await
    }

    async fn head_seq(&self) -> Result<u64, StoreError> {
        self.inner.head_seq().await
    }
}

#[async_trait]
impl<S: SweepStore> EffectRead for CrashingStore<S> {
    async fn effect(
        &self,
        id: domovoi_core::EffectId,
    ) -> Result<Option<domovoi_core::ports::EffectRecord>, StoreError> {
        self.inner.effect(id).await
    }

    async fn effects_with_status(
        &self,
        status: domovoi_core::ports::EffectStatus,
        limit: u16,
    ) -> Result<Vec<domovoi_core::ports::EffectRecord>, StoreError> {
        self.inner.effects_with_status(status, limit).await
    }
}

/// Reopens durable storage after a simulated crash.
#[async_trait]
pub trait Reopen<S>: Send + Sync {
    /// Closes `old` if the backend needs it and returns the store as a freshly started
    /// process would see it. In memory: the same instance. SQLite: close the pools and
    /// open the file again.
    async fn reopen(&self, old: Arc<S>) -> Arc<S>;
}

/// Reopens nothing: the in-memory store survives as the same instance.
pub struct InMemoryReopen;

#[async_trait]
impl Reopen<crate::InMemoryStore> for InMemoryReopen {
    async fn reopen(&self, old: Arc<crate::InMemoryStore>) -> Arc<crate::InMemoryStore> {
        old
    }
}
