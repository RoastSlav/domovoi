//! Event-bus consumer and per-incident delivery ordering.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    Clock, IncidentFilter, IncidentRepo, Notifier, NotifyStateRepo, OutboundMessage, Page,
};
use domovoi_core::{Incident, IncidentId, IncidentStatus, NotifyState};
use domovoi_engine::events::DomainEvent;
use domovoi_engine::notify::{self, NotifyAction, NotifySettings};
use domovoi_store::Store;
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::delivery::Delivery;

/// The incident an action belongs to.
fn incident_of(action: &NotifyAction) -> IncidentId {
    match action {
        NotifyAction::PostCard { incident }
        | NotifyAction::UpdateCard { incident }
        | NotifyAction::ThreadReply { incident, .. }
        | NotifyAction::Hold { incident, .. } => *incident,
    }
}

/// Runs each incident's deliveries one after another, in the order they were submitted,
/// while different incidents proceed in parallel.
#[derive(Default)]
struct Sequencer {
    /// Per incident, the signal that its latest submitted job has finished.
    tails: Mutex<BTreeMap<IncidentId, oneshot::Receiver<()>>>,
}

impl Sequencer {
    /// Queues a job for `incident`: what to wait for first, and what to signal when done.
    fn enqueue(
        &self,
        incident: IncidentId,
    ) -> (Option<oneshot::Receiver<()>>, oneshot::Sender<()>) {
        let (done, tail) = oneshot::channel();
        let mut tails = self
            .tails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Forget incidents whose last job has finished, so the map holds only busy ones.
        tails.retain(|_, tail| matches!(tail.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
        (tails.insert(incident, tail), done)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.tails
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

/// Runs `job` after `previous`, then signals `done`. A previous job that was cancelled
/// still releases the next one.
async fn after(
    previous: Option<oneshot::Receiver<()>>,
    done: oneshot::Sender<()>,
    job: impl Future<Output = ()>,
) {
    if let Some(previous) = previous {
        let _ = previous.await;
    }
    job.await;
    let _ = done.send(());
}

/// Delivers domain events and time-driven notification plans.
#[derive(Clone)]
pub(crate) struct Dispatcher {
    clock: Arc<dyn Clock>,
    store: Arc<Store>,
    settings: NotifySettings,
    delivery: Delivery,
    sequencer: Arc<Sequencer>,
}

impl Dispatcher {
    pub(crate) fn new(
        config: Arc<HubConfig>,
        clock: Arc<dyn Clock>,
        store: Arc<Store>,
        gateways: Vec<Arc<dyn domovoi_core::ports::ChatGateway>>,
        notifiers: Vec<Arc<dyn Notifier>>,
        settings: NotifySettings,
    ) -> Self {
        let delivery = Delivery::new(
            config,
            clock.clone(),
            store.clone(),
            gateways,
            notifiers,
            settings.clone(),
        );
        Self {
            clock,
            store,
            settings,
            delivery,
            sequencer: Arc::new(Sequencer::default()),
        }
    }

    /// Queues `job` for `incident` on `running`, behind that incident's earlier jobs.
    fn submit(
        &self,
        running: &mut JoinSet<()>,
        incident: IncidentId,
        job: impl Future<Output = ()> + Send + 'static,
    ) {
        let (previous, done) = self.sequencer.enqueue(incident);
        running.spawn(after(previous, done, job));
    }

    /// Runs `job` for `incident` now, behind that incident's earlier jobs.
    async fn in_order(&self, incident: IncidentId, job: impl Future<Output = ()>) {
        let (previous, done) = self.sequencer.enqueue(incident);
        after(previous, done, job).await;
    }

    /// Consumes events until cancelled. Lag rebuilds open incidents' cards from the store.
    pub(crate) async fn run(
        self,
        mut events: broadcast::Receiver<DomainEvent>,
        shutdown: CancellationToken,
    ) {
        let mut running = JoinSet::new();
        loop {
            // Forget finished deliveries, so the set holds only work still in flight.
            while running.try_join_next().is_some() {}
            let event = tokio::select! {
                () = shutdown.cancelled() => break,
                event = events.recv() => event,
            };
            match event {
                Ok(event) => {
                    let dispatcher = self.clone();
                    let incident = event.incident.id();
                    self.submit(&mut running, incident, async move {
                        dispatcher.delivery.event(event).await;
                    });
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    metrics::counter!(
                        "domovoi_event_bus_lagged_total",
                        "consumer" => "dispatcher"
                    )
                    .increment(missed);
                    tracing::warn!(
                        missed,
                        "delivery missed events; rebuilding open incidents' cards from the store"
                    );
                    self.recover(&mut running).await;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        while running.join_next().await.is_some() {}
    }

    /// Queues a card refresh for every open, confirmed incident.
    async fn recover(&self, running: &mut JoinSet<()>) {
        let open = IncidentFilter {
            active: Some(true),
            ..IncidentFilter::default()
        };
        let mut page = Page::first(Page::MAX_LIMIT);
        loop {
            let got = match IncidentRepo::list(&*self.store, &open, &page).await {
                Ok(got) => got,
                Err(err) => {
                    tracing::warn!(%err, "could not reload incidents after missed events");
                    return;
                }
            };
            for incident in got.items {
                if incident.status() == IncidentStatus::Pending {
                    continue;
                }
                let delivery = self.delivery.clone();
                let id = incident.id();
                self.submit(running, id, async move { delivery.recover(incident).await });
            }
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => break,
            }
        }
    }

    /// Runs reminders and quiet-hours release once.
    pub(crate) async fn tick(&self) {
        let items = self.notification_items().await;
        let plan = notify::tick(&items, &self.settings, self.clock.now());
        let before: BTreeMap<IncidentId, NotifyState> = items
            .iter()
            .map(|(incident, state)| (incident.id(), state.clone()))
            .collect();
        let after: BTreeMap<IncidentId, NotifyState> = plan.states.iter().cloned().collect();
        let released: Vec<IncidentId> = plan
            .states
            .iter()
            .filter(|(id, state)| {
                !state.is_held() && before.get(id).is_some_and(NotifyState::is_held)
            })
            .map(|(id, _)| *id)
            .collect();

        // Reminders come first. One that is not delivered leaves `last_reminder_at` where it
        // was, so the next tick plans it again instead of losing it.
        for (id, state) in &after {
            if released.contains(id) {
                continue;
            }
            let Some((incident, _)) = items.iter().find(|(incident, _)| incident.id() == *id)
            else {
                continue;
            };
            let Some(old) = before.get(id) else { continue };
            let actions: Vec<NotifyAction> = plan
                .actions
                .iter()
                .filter(|action| incident_of(action) == *id)
                .cloned()
                .collect();
            if actions.is_empty() {
                self.delivery.save_state(*id, state).await;
                continue;
            }
            self.in_order(*id, async {
                self.delivery
                    .tick_actions(incident.clone(), old.clone(), state.clone(), actions)
                    .await;
            })
            .await;
        }

        let Some(text) = plan.release else {
            // Nothing was released, so nothing leaves the held queue this tick. Should a
            // state do so anyway, it is kept as it is: there is nothing to confirm.
            for id in released {
                if let Some(state) = after.get(&id) {
                    self.delivery.save_state(id, state).await;
                }
            }
            return;
        };
        let summary_due = self.delivery.can_replace_held_cards();
        let summary_sent = summary_due
            && self
                .delivery
                .send_all(OutboundMessage {
                    title: "While you were away".to_owned(),
                    markdown: text,
                    severity: None,
                })
                .await
                > 0;
        // Every incident still open gets a card, because its buttons and reminders hang off
        // it. One that resolved while held needs a card only when its summary failed, or
        // nothing would say it happened (INV-11).
        let summary_failed = summary_due && !summary_sent;
        for id in released {
            let Some((incident, _)) = items.iter().find(|(incident, _)| incident.id() == id) else {
                continue;
            };
            let (Some(old), Some(state)) = (before.get(&id), after.get(&id)) else {
                continue;
            };
            if incident.resolved_at().is_some() && !summary_failed {
                self.delivery.save_state(id, state).await;
                continue;
            }
            // A card or summary that is not delivered leaves the incident held, so the next
            // tick releases it again rather than losing it.
            let action = NotifyAction::PostCard { incident: id };
            self.in_order(id, async {
                self.delivery
                    .tick_actions(incident.clone(), old.clone(), state.clone(), vec![action])
                    .await;
            })
            .await;
        }
    }

    /// Sends one already-rendered digest through every notifier; `true` when at least one
    /// accepted it.
    pub(crate) async fn digest(&self, date: String, markdown: String) -> bool {
        self.delivery
            .send_all(OutboundMessage {
                title: format!("Daily digest for {date}"),
                markdown,
                severity: None,
            })
            .await
            > 0
    }

    /// Whether any one-way notifier is configured, for digests and quiet-hours summaries.
    pub(crate) fn has_notifiers(&self) -> bool {
        self.delivery.has_notifiers()
    }

    async fn notification_items(&self) -> Vec<(Incident, NotifyState)> {
        let mut items = BTreeMap::new();
        let filter = IncidentFilter {
            active: Some(true),
            ..IncidentFilter::default()
        };
        let mut page = Page::first(Page::MAX_LIMIT);
        loop {
            let got = match self.store.list(&filter, &page).await {
                Ok(got) => got,
                Err(err) => {
                    tracing::warn!(%err, "could not load incidents for reminders");
                    return Vec::new();
                }
            };
            for incident in got.items {
                // An unreadable row for one incident must not be replaced by a default: it
                // would plan the alert as if nothing had been said yet.
                match NotifyStateRepo::get(&*self.store, incident.id()).await {
                    Ok(Some(state)) => {
                        items.insert(incident.id(), (incident, state));
                    }
                    Ok(None) => {}
                    Err(err) => {
                        tracing::error!(
                            %err,
                            incident = %incident.id(),
                            "could not read notification state; skipping this incident"
                        );
                        super::delivery::state_error("read");
                    }
                }
            }
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => break,
            }
        }
        match NotifyStateRepo::list_held(&*self.store).await {
            Ok(held) => {
                for (id, state) in held {
                    if let Ok(Some(incident)) = IncidentRepo::get(&*self.store, id).await {
                        items.insert(id, (incident, state));
                    }
                }
            }
            Err(err) => {
                tracing::error!(%err, "could not read held notification states");
                super::delivery::state_error("read");
            }
        }
        items.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domovoi_core::IncidentId;
    use tokio::sync::{Mutex, Notify};
    use tokio::task::JoinSet;

    use super::{Sequencer, after};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_incidents_jobs_run_in_submission_order() {
        let sequencer = Sequencer::default();
        let incident = IncidentId::new();
        let other = IncidentId::new();
        let order = Arc::new(Mutex::new(Vec::new()));
        let gate = Arc::new(Notify::new());
        let mut running = JoinSet::new();
        for n in 0..20 {
            let (previous, done) = sequencer.enqueue(incident);
            let order = order.clone();
            let gate = gate.clone();
            running.spawn(after(previous, done, async move {
                if n == 0 {
                    // The first job waits; nothing later may overtake it.
                    gate.notified().await;
                }
                order.lock().await.push(n);
            }));
        }
        let (previous, done) = sequencer.enqueue(other);
        let other_done = Arc::new(Notify::new());
        let signal = other_done.clone();
        running.spawn(after(previous, done, async move { signal.notify_one() }));
        other_done.notified().await;
        assert!(
            order.lock().await.is_empty(),
            "another incident is not held up"
        );

        gate.notify_one();
        while running.join_next().await.is_some() {}
        assert_eq!(*order.lock().await, (0..20).collect::<Vec<_>>());

        let _ = sequencer.enqueue(IncidentId::new());
        assert_eq!(sequencer.len(), 1, "finished incidents are forgotten");
    }
}
