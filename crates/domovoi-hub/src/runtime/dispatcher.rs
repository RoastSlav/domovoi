//! Event-bus consumer and per-incident delivery serialization.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    Clock, IncidentFilter, IncidentRepo, Notifier, NotifyStateRepo, OutboundMessage, Page,
};
use domovoi_core::{Incident, IncidentId, NotifyState};
use domovoi_engine::events::DomainEvent;
use domovoi_engine::notify::{self, NotifySettings};
use domovoi_store::Store;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::delivery::Delivery;

type IncidentLock = Arc<tokio::sync::Mutex<()>>;

/// Delivers domain events and time-driven notification plans.
#[derive(Clone)]
pub(crate) struct Dispatcher {
    clock: Arc<dyn Clock>,
    store: Arc<Store>,
    settings: NotifySettings,
    delivery: Delivery,
    locks: Arc<Mutex<BTreeMap<IncidentId, IncidentLock>>>,
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
            locks: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Consumes events until cancelled. Lag rebuilds current cards from the store.
    pub(crate) async fn run(
        self,
        mut events: broadcast::Receiver<DomainEvent>,
        shutdown: CancellationToken,
    ) {
        let mut running = tokio::task::JoinSet::new();
        loop {
            let event = tokio::select! {
                () = shutdown.cancelled() => break,
                event = events.recv() => event,
            };
            match event {
                Ok(event) => {
                    let dispatcher = self.clone();
                    running.spawn(async move { dispatcher.event(event).await });
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    metrics::counter!(
                        "domovoi_event_bus_lagged_total",
                        "consumer" => "dispatcher"
                    )
                    .increment(missed);
                    tracing::warn!(
                        missed,
                        "delivery missed events; rebuilding cards from the store"
                    );
                    self.delivery.recover().await;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        while running.join_next().await.is_some() {}
    }

    async fn event(&self, event: DomainEvent) {
        let lock = self.lock(event.incident.id());
        let _guard = lock.lock().await;
        self.delivery.event(event).await;
    }

    /// Runs reminders and quiet-hours release once.
    pub(crate) async fn tick(&self) {
        let items = self.notification_items().await;
        let plan = notify::tick(&items, &self.settings, self.clock.now());
        for (incident, state) in plan.states {
            if let Err(err) =
                NotifyStateRepo::put(&*self.store, incident, &state, self.clock.now()).await
            {
                tracing::warn!(%err, %incident, "could not save a notification tick");
            }
        }
        for action in plan.actions {
            let id = match &action {
                notify::NotifyAction::PostCard { incident }
                | notify::NotifyAction::UpdateCard { incident }
                | notify::NotifyAction::ThreadReply { incident, .. }
                | notify::NotifyAction::Hold { incident, .. } => *incident,
            };
            let Some(incident) = items.iter().find(|(incident, _)| incident.id() == id) else {
                continue;
            };
            let lock = self.lock(id);
            let _guard = lock.lock().await;
            self.delivery
                .tick_actions(incident.0.clone(), vec![action])
                .await;
        }
        if let Some(text) = plan.release {
            self.delivery
                .send_all(OutboundMessage {
                    title: "While you were away".to_owned(),
                    markdown: text,
                    severity: None,
                })
                .await;
        }
    }

    /// Sends one already-rendered digest through every notifier.
    pub(crate) async fn digest(&self, date: String, markdown: String) {
        self.delivery
            .send_all(OutboundMessage {
                title: format!("Daily digest for {date}"),
                markdown,
                severity: None,
            })
            .await;
    }

    fn lock(&self, incident: IncidentId) -> IncidentLock {
        self.locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(incident)
            .or_default()
            .clone()
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
