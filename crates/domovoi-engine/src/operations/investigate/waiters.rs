//! Callers waiting for the first outcome an investigation attaches to their incident.

use std::collections::BTreeMap;
use std::sync::Mutex;

use domovoi_core::{IncidentId, TriageOutcome};
use tokio::sync::oneshot;

/// Waiting callers by incident. In memory only: a caller that is gone after a restart simply
/// no longer waits, and the investigation still attaches its outcome.
#[derive(Default)]
pub(crate) struct Waiters {
    inner: Mutex<Registry>,
}

/// The senders by incident, each with the id of the call that registered it, and the next id.
#[derive(Default)]
struct Registry {
    next: u64,
    by_incident: BTreeMap<IncidentId, Vec<(u64, oneshot::Sender<TriageOutcome>)>>,
}

impl Waiters {
    /// Starts waiting for the next outcome attached to `incident`.
    pub(crate) fn register(&self, incident: IncidentId) -> (u64, oneshot::Receiver<TriageOutcome>) {
        let (tx, rx) = oneshot::channel();
        let mut registry = self.lock();
        registry.next += 1;
        let id = registry.next;
        registry
            .by_incident
            .entry(incident)
            .or_default()
            .push((id, tx));
        (id, rx)
    }

    /// Hands `outcome` to everyone waiting on `incident`.
    pub(crate) fn resolve(&self, incident: IncidentId, outcome: &TriageOutcome) {
        let senders = self
            .lock()
            .by_incident
            .remove(&incident)
            .unwrap_or_default();
        for (_, sender) in senders {
            let _ = sender.send(outcome.clone());
        }
    }

    /// Stops waiting for registration `id`; other callers waiting on `incident` keep theirs.
    pub(crate) fn forget(&self, incident: IncidentId, id: u64) {
        let mut registry = self.lock();
        if let Some(list) = registry.by_incident.get_mut(&incident) {
            list.retain(|(held, _)| *held != id);
            if list.is_empty() {
                registry.by_incident.remove(&incident);
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Registry> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::IncidentId;

    use super::*;

    #[test]
    fn leaving_one_waiter_keeps_the_other_waiting() {
        let waiters = Waiters::default();
        let incident = IncidentId::new();
        let (first, mut first_rx) = waiters.register(incident);
        let (second, _second_rx) = waiters.register(incident);
        waiters.forget(incident, second);
        let outcome = TriageOutcome::Skipped {
            reason: "x".to_owned(),
        };
        waiters.resolve(incident, &outcome);
        assert_eq!(first_rx.try_recv().ok(), Some(outcome));
        let _ = first;
    }
}
