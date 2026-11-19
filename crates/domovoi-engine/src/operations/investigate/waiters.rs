//! Callers waiting for the first outcome an investigation attaches to their incident.

use std::collections::BTreeMap;
use std::sync::Mutex;

use domovoi_core::{IncidentId, TriageOutcome};
use tokio::sync::oneshot;

/// Waiting callers by incident. In memory only: a caller that is gone after a restart simply
/// no longer waits, and the investigation still attaches its outcome.
#[derive(Default)]
pub(crate) struct Waiters {
    inner: Mutex<BTreeMap<IncidentId, Vec<oneshot::Sender<TriageOutcome>>>>,
}

impl Waiters {
    /// Starts waiting for the next outcome attached to `incident`.
    pub(crate) fn register(&self, incident: IncidentId) -> oneshot::Receiver<TriageOutcome> {
        let (tx, rx) = oneshot::channel();
        self.lock().entry(incident).or_default().push(tx);
        rx
    }

    /// Hands `outcome` to everyone waiting on `incident`.
    pub(crate) fn resolve(&self, incident: IncidentId, outcome: &TriageOutcome) {
        let senders = self.lock().remove(&incident).unwrap_or_default();
        for sender in senders {
            let _ = sender.send(outcome.clone());
        }
    }

    /// Stops waiting on `incident`: used when the call that registered it gives up.
    pub(crate) fn forget(&self, incident: IncidentId) {
        self.lock().remove(&incident);
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, BTreeMap<IncidentId, Vec<oneshot::Sender<TriageOutcome>>>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
