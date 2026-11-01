//! The bounded request queue and the workers that drain it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use domovoi_core::{Incident, TriageOutcome};
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinSet;

use super::{Shared, run};
use crate::events::{DomainEvent, EventKind};

/// Incidents waiting for a worker, oldest first. A full queue drops the oldest.
pub(crate) struct Queue {
    items: Mutex<VecDeque<Incident>>,
    ready: Notify,
    capacity: usize,
}

impl Queue {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            items: Mutex::new(VecDeque::new()),
            ready: Notify::new(),
            capacity,
        }
    }

    fn items(&self) -> std::sync::MutexGuard<'_, VecDeque<Incident>> {
        self.items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Queues `incident`, replacing a request for the same incident still waiting. Returns
    /// the oldest request when the queue was full.
    pub(crate) fn push(&self, incident: Incident) -> Option<Incident> {
        let dropped = {
            let mut items = self.items();
            if let Some(waiting) = items.iter_mut().find(|i| i.id() == incident.id()) {
                *waiting = incident;
                None
            } else {
                items.push_back(incident);
                (items.len() > self.capacity)
                    .then(|| items.pop_front())
                    .flatten()
            }
        };
        self.ready.notify_one();
        dropped
    }

    fn pop(&self) -> Option<Incident> {
        self.items().pop_front()
    }

    pub(crate) fn len(&self) -> usize {
        self.items().len()
    }
}

/// Feeds `TriageRequested` events into the queue and runs the workers until shutdown.
/// Requests for incidents in a maintenance window or under an ack rule (`notify == false`)
/// are not triaged: nobody is told about them.
pub(crate) async fn serve(shared: Arc<Shared>, mut events: broadcast::Receiver<DomainEvent>) {
    let mut workers = JoinSet::new();
    for _ in 0..shared.settings.concurrency.max(1) {
        workers.spawn(worker(shared.clone()));
    }
    let shutdown = shared.deps.shutdown.clone();
    loop {
        let event = tokio::select! {
            () = shutdown.cancelled() => break,
            event = events.recv() => event,
        };
        match event {
            Ok(event) if event.kind == EventKind::TriageRequested && event.notify => {
                if let Some(dropped) = shared.queue.push(event.incident) {
                    skip_dropped(&shared, &dropped).await;
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "triage missed events; some incidents go untriaged");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    shutdown.cancel();
    while workers.join_next().await.is_some() {}
}

async fn skip_dropped(shared: &Shared, dropped: &Incident) {
    let outcome = TriageOutcome::Skipped {
        reason: "too many incidents waiting for triage".to_owned(),
    };
    if let Err(err) = shared
        .deps
        .manager
        .attach_triage(dropped.id(), outcome, BTreeMap::new())
        .await
    {
        tracing::warn!(%err, incident = %dropped.id(), "could not record a skipped triage");
    }
}

async fn worker(shared: Arc<Shared>) {
    let shutdown = shared.deps.shutdown.clone();
    loop {
        if let Some(incident) = shared.queue.pop() {
            tokio::select! {
                () = shutdown.cancelled() => return,
                _ = run::triage(&shared, &incident) => {}
            }
            continue;
        }
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = shared.queue.ready.notified() => {}
        }
    }
}
