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
        let released: Vec<IncidentId> = plan
            .states
            .iter()
            .filter(|(id, state)| {
                !state.is_held()
                    && items
                        .iter()
                        .any(|(incident, before)| incident.id() == *id && before.is_held())
            })
            .map(|(id, _)| *id)
            .collect();
        for (incident, state) in &plan.states {
            if let Err(err) =
                NotifyStateRepo::put(&*self.store, *incident, state, self.clock.now()).await
            {
                tracing::warn!(%err, %incident, "could not save a notification tick");
            }
        }
        for action in plan.actions {
            let id = match &action {
                NotifyAction::PostCard { incident }
                | NotifyAction::UpdateCard { incident }
                | NotifyAction::ThreadReply { incident, .. }
                | NotifyAction::Hold { incident, .. } => *incident,
            };
            let Some((incident, _)) = items.iter().find(|(incident, _)| incident.id() == id) else {
                continue;
            };
            let incident = incident.clone();
            self.in_order(id, self.delivery.tick_actions(incident, vec![action]))
                .await;
        }
        let Some(text) = plan.release else {
            return;
        };
        if self.delivery.has_notifiers() {
            self.delivery
                .send_all(OutboundMessage {
                    title: "While you were away".to_owned(),
                    markdown: text,
                    severity: None,
                })
                .await;
            return;
        }
        // No notifier for the summary: post a card for each held incident still open, so
        // nothing raised during quiet hours is lost (INV-11).
        for id in released {
            let Some((incident, _)) = items.iter().find(|(incident, _)| incident.id() == id) else {
                continue;
            };
            if incident.resolved_at().is_some() {
                continue;
            }
            let action = NotifyAction::PostCard { incident: id };
            self.in_order(
                id,
                self.delivery.tick_actions(incident.clone(), vec![action]),
            )
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
                if let Ok(Some(state)) = NotifyStateRepo::get(&*self.store, incident.id()).await {
                    items.insert(incident.id(), (incident, state));
                }
            }
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => break,
            }
        }
        if let Ok(held) = NotifyStateRepo::list_held(&*self.store).await {
            for (id, state) in held {
                if let Ok(Some(incident)) = IncidentRepo::get(&*self.store, id).await {
                    items.insert(id, (incident, state));
                }
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
