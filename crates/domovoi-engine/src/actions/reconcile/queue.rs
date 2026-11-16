//! Which hosts reconcile, how many at once, and what happens to a trigger that arrives
//! while a pass is already running.
//!
//! A trigger is never dropped. One that arrives before a pass starts joins the waiting set;
//! one that arrives during a pass marks the host as needing another, and the pass queues it
//! when it releases its gate. The extra pass is harmless when the first one already covered
//! the trigger: it reads the pending rows again and finds nothing left to settle.
//!
//! Passes for different hosts run in bounds, so a host whose run keeps answering "running"
//! for hours cannot keep another host's recovery waiting. The bound is a small constant: a
//! pass is idle most of the time (one query, then the clock), so a handful of them keeps a
//! home lab responsive without opening a task per host or per trigger.

use std::sync::Arc;
use std::sync::PoisonError;

use domovoi_core::{ActionId, HostId};
use tokio::sync::OwnedMutexGuard;
use tokio::task::JoinSet;

use super::Shared;
use super::answer::{HUB_NOT_HANDED_OFF_TOKEN, ReconcileOutcome};
use crate::actions::writer::WriteCommand;

/// How many hosts may reconcile at the same time.
pub const MAX_RECONCILE_PASSES: usize = 4;

/// A host's whole-pass gate, released when the pass ends however it ends: a finished pass,
/// a cancelled session, or an aborted task. Releasing it also starts the follow-up pass a
/// trigger asked for while this one was reading.
pub(crate) struct PassHold {
    shared: Arc<Shared>,
    host: HostId,
    gate: Option<OwnedMutexGuard<()>>,
}

impl Drop for PassHold {
    fn drop(&mut self) {
        // The gate goes first: a trigger racing this release then either takes the free gate
        // itself or records itself before the check below, so it can never be stranded.
        drop(self.gate.take());
        self.shared.pass_finished(self.host);
    }
}

/// Waits for the host's pass gate, so a pass asked for by a session runs after any pass
/// already reading that host.
pub(crate) async fn hold_pass(shared: Arc<Shared>, host: HostId) -> PassHold {
    let gate = pass_gate_arc(&shared, host).lock_owned().await;
    PassHold {
        shared,
        host,
        gate: Some(gate),
    }
}

/// Takes the host's pass gate if no pass holds it. A caller that gets `None` records the
/// host with [`Shared::defer_pass`] instead of polling.
pub(crate) fn try_hold_pass(shared: Arc<Shared>, host: HostId) -> Option<PassHold> {
    let gate = pass_gate_arc(&shared, host).try_lock_owned().ok()?;
    Some(PassHold {
        shared,
        host,
        gate: Some(gate),
    })
}

impl Shared {
    /// Records that `host` needs another pass after the one that is running now. The running
    /// pass calls [`Shared::pass_finished`] when it releases its gate and queues it then.
    pub(crate) fn defer_pass(&self, host: HostId) {
        lock(&self.reconcile_again).insert(host);
        // The pass may have released its gate between the caller's failed `try_lock` and
        // this insertion, in which case its own check already ran. Probe the gate: when it
        // is free there is no pass left to notice, so queue the host here.
        let free = match pass_gate_arc(self, host).try_lock_owned() {
            Ok(gate) => {
                drop(gate);
                true
            }
            Err(_) => false,
        };
        if free {
            lock(&self.reconcile_again).remove(&host);
            self.schedule_reconciliation(host);
        }
    }

    /// Ends a pass: starts the single follow-up pass a trigger asked for meanwhile, if any.
    fn pass_finished(&self, host: HostId) {
        if lock(&self.reconcile_again).remove(&host) {
            self.schedule_reconciliation(host);
        }
    }
}

/// The host's pass gate, shared so a holder can be chosen.
fn pass_gate_arc(shared: &Shared, host: HostId) -> Arc<tokio::sync::Mutex<()>> {
    let mut gates = lock(&shared.reconcile_passes);
    gates
        .entry(host)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Serves reconciliation asked for by the action service itself: outcomes that were lost
/// while the agent stayed connected would otherwise wait for a reconnect that never comes.
///
/// Each host pass is its own task, and the scheduler never holds a host's gate itself for
/// long: a gate that is busy makes the scheduler record the host for the running pass and
/// move on. A burst of triggers therefore starts at most one waiting entry per host and
/// never a task per trigger.
pub(crate) async fn serve(shared: Arc<Shared>) {
    let shutdown = shared.deps.shutdown.clone();
    let mut passes: JoinSet<()> = JoinSet::new();
    loop {
        while passes.try_join_next().is_some() {}
        // Start what fits. A slot that is not free leaves its host waiting in the set, so a
        // full pool delays work instead of losing it.
        while let Some(host) = next_pending(&shared) {
            let Ok(permit) = shared.reconcile_slots.clone().try_acquire_owned() else {
                lock(&shared.reconcile_pending).insert(host);
                break;
            };
            let Some(hold) = try_hold_pass(shared.clone(), host) else {
                drop(permit);
                shared.defer_pass(host);
                continue;
            };
            let worker = shared.clone();
            passes.spawn(async move {
                let _permit = permit;
                let _hold = hold;
                super::pass(&worker, host).await;
            });
        }
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = shared.reconcile_nudge.notified() => {}
            Some(_) = passes.join_next() => {}
        }
    }
    // A pass aborted here drops its hold, which releases the gate and queues any follow-up
    // it was asked for; the shutdown below then finishes off whatever is left.
    passes.shutdown().await;
}

/// Takes the next host waiting for a pass.
fn next_pending(shared: &Shared) -> Option<HostId> {
    let mut pending = lock(&shared.reconcile_pending);
    let host = pending.iter().next().copied()?;
    pending.remove(&host);
    Some(host)
}

fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records a run the hub provably never handed to the agent as one that never started.
///
/// `start_skill` only answers with an error before the frame enters the session's outgoing
/// queue, so this proof is the hub's own and needs no journal answer, no reconnect and no
/// `authoritative_absence` promise. It goes through the writer like every other decision, so
/// a terminal decision made meanwhile still wins.
pub(crate) async fn handoff_failed(shared: &Shared, id: ActionId) {
    let (reply, answer) = tokio::sync::oneshot::channel();
    let command = WriteCommand::Reconcile {
        id,
        outcome: ReconcileOutcome::HandoffFailed,
        wire_reason: HUB_NOT_HANDED_OFF_TOKEN,
        reply,
    };
    if shared.writer.tx.send(command).await.is_err() {
        tracing::debug!(
            action = %id,
            "the action service is closing; the failed hand-off is not recorded",
        );
        return;
    }
    if answer.await.is_err() {
        tracing::debug!(
            action = %id,
            "the writer stopped before answering about a failed hand-off",
        );
    }
}
