//! Chat delivery, reference persistence and bounded retries.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    ActionFilter, ActionRepo, AdvisoryRepo, ChatError, ChatGateway, Clock, HostRepo,
    IncidentFilter, IncidentRepo, MessageRef, Notifier, NotifyStateRepo, OutboundMessage, Page,
    SignalRepo, StoreError,
};
use domovoi_core::{
    Advisory, AdvisoryKind, Card, ChatRefs, Incident, IncidentId, NotifyState, TriageOutcome,
};
use domovoi_engine::advisories::advisory_id;
use domovoi_engine::cards::{CardLinks, build_card};
use domovoi_engine::events::{DomainEvent, EventKind};
use domovoi_engine::notify::{self, NotifyAction, NotifySettings};
use domovoi_store::Store;
use jiff::SignedDuration;

const BACKOFF: [i64; 5] = [1, 2, 4, 8, 16];

#[derive(Clone)]
pub(super) struct Delivery {
    config: Arc<HubConfig>,
    clock: Arc<dyn Clock>,
    store: Arc<Store>,
    gateways: Vec<Arc<dyn ChatGateway>>,
    notifiers: Vec<Arc<dyn Notifier>>,
    settings: NotifySettings,
    failures: Arc<Mutex<BTreeMap<String, u32>>>,
}

impl Delivery {
    pub(super) fn new(
        config: Arc<HubConfig>,
        clock: Arc<dyn Clock>,
        store: Arc<Store>,
        gateways: Vec<Arc<dyn ChatGateway>>,
        notifiers: Vec<Arc<dyn Notifier>>,
        settings: NotifySettings,
    ) -> Self {
        Self {
            config,
            clock,
            store,
            gateways,
            notifiers,
            settings,
            failures: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub(super) async fn event(&self, event: DomainEvent) {
        let id = event.incident.id();
        let state = self.state(id).await;
        let plan = notify::plan(&event, &state, &self.settings, self.clock.now());
        if let Err(err) =
            NotifyStateRepo::put(&*self.store, id, &plan.state, self.clock.now()).await
        {
            tracing::warn!(%err, %id, "could not save notification policy state");
        }
        let tokens = match event.kind {
            EventKind::TriageCompleted { tokens, .. } => tokens,
            _ => BTreeMap::new(),
        };
        self.actions(event.incident, plan.actions, plan.state, tokens)
            .await;
    }

    pub(super) async fn tick_actions(&self, incident: Incident, actions: Vec<NotifyAction>) {
        let id = incident.id();
        let state = self.state(id).await;
        self.actions(incident, actions, state, BTreeMap::new())
            .await;
    }

    async fn actions(
        &self,
        incident: Incident,
        actions: Vec<NotifyAction>,
        state: NotifyState,
        tokens: BTreeMap<domovoi_core::ActionId, String>,
    ) {
        if actions.is_empty() || self.gateways.is_empty() {
            return;
        }
        let Some(card) = self.card(&incident, tokens).await else {
            return;
        };
        let state = Arc::new(tokio::sync::Mutex::new(state));
        let jobs = self.gateways.iter().map(|gateway| {
            let state = state.clone();
            let actions = actions.clone();
            let card = card.clone();
            let incident = incident.clone();
            async move {
                for action in actions {
                    self.perform(gateway, &incident, &card, &state, action)
                        .await;
                }
            }
        });
        futures::future::join_all(jobs).await;
    }

    async fn perform(
        &self,
        gateway: &Arc<dyn ChatGateway>,
        incident: &Incident,
        card: &Card,
        state: &Arc<tokio::sync::Mutex<NotifyState>>,
        action: NotifyAction,
    ) {
        let platform = gateway.platform().to_owned();
        let result = match action {
            NotifyAction::PostCard { .. } => {
                self.post(gateway, incident, card, state).await.map(|_| ())
            }
            NotifyAction::UpdateCard { .. } => self.update(gateway, incident, card, state).await,
            NotifyAction::ThreadReply { text, .. } => {
                self.reply(gateway, incident, card, state, &text).await
            }
            NotifyAction::Hold { .. } => Ok(()),
        };
        match result {
            Ok(()) => self.succeeded(&platform).await,
            Err(err) => {
                tracing::warn!(%err, gateway = %platform, incident = %incident.id(), "delivery failed after retries");
                self.failed(&platform).await;
            }
        }
    }

    async fn post(
        &self,
        gateway: &Arc<dyn ChatGateway>,
        incident: &Incident,
        card: &Card,
        state: &Arc<tokio::sync::Mutex<NotifyState>>,
    ) -> Result<MessageRef, ChatError> {
        if let Some(message) = self.message(state, gateway.platform()).await {
            return Ok(message);
        }
        let message = self.retry(|| gateway.post_card(card)).await?;
        {
            let mut current = state.lock().await;
            current.messages.insert(
                gateway.platform().to_owned(),
                ChatRefs {
                    message: message.clone(),
                    thread: None,
                },
            );
            self.save(incident.id(), &current).await;
        }
        let thread = self
            .retry(|| gateway.open_thread(&message, incident.title()))
            .await?;
        let mut current = state.lock().await;
        if let Some(refs) = current.messages.get_mut(gateway.platform()) {
            refs.thread = Some(thread);
        }
        self.save(incident.id(), &current).await;
        Ok(message)
    }

    async fn update(
        &self,
        gateway: &Arc<dyn ChatGateway>,
        incident: &Incident,
        card: &Card,
        state: &Arc<tokio::sync::Mutex<NotifyState>>,
    ) -> Result<(), ChatError> {
        let Some(message) = self.message(state, gateway.platform()).await else {
            return self.post(gateway, incident, card, state).await.map(|_| ());
        };
        match self.retry(|| gateway.update_card(&message, card)).await {
            Ok(()) => Ok(()),
            Err(ChatError::NotFound) => {
                state.lock().await.messages.remove(gateway.platform());
                self.post(gateway, incident, card, state).await.map(|_| ())
            }
            Err(err) => Err(err),
        }
    }

    async fn reply(
        &self,
        gateway: &Arc<dyn ChatGateway>,
        incident: &Incident,
        card: &Card,
        state: &Arc<tokio::sync::Mutex<NotifyState>>,
        text: &str,
    ) -> Result<(), ChatError> {
        let message = self.post(gateway, incident, card, state).await?;
        let existing = state
            .lock()
            .await
            .messages
            .get(gateway.platform())
            .and_then(|refs| refs.thread.clone());
        let thread = match existing {
            Some(thread) => thread,
            None => {
                let thread = self
                    .retry(|| gateway.open_thread(&message, incident.title()))
                    .await?;
                let mut current = state.lock().await;
                if let Some(refs) = current.messages.get_mut(gateway.platform()) {
                    refs.thread = Some(thread.clone());
                }
                self.save(incident.id(), &current).await;
                thread
            }
        };
        self.retry(|| gateway.reply(&thread, text)).await
    }

    async fn message(
        &self,
        state: &Arc<tokio::sync::Mutex<NotifyState>>,
        platform: &str,
    ) -> Option<MessageRef> {
        state
            .lock()
            .await
            .messages
            .get(platform)
            .map(|refs| refs.message.clone())
    }

    async fn save(&self, incident: IncidentId, state: &NotifyState) {
        if let Err(err) =
            NotifyStateRepo::put(&*self.store, incident, state, self.clock.now()).await
        {
            tracing::warn!(%err, %incident, "could not save chat message references");
        }
    }

    async fn state(&self, incident: IncidentId) -> NotifyState {
        NotifyStateRepo::get(&*self.store, incident)
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    async fn card(
        &self,
        incident: &Incident,
        tokens: BTreeMap<domovoi_core::ActionId, String>,
    ) -> Option<Card> {
        let host = match self.store.get_host(incident.host()).await {
            Ok(Some(host)) => host,
            Ok(None) => {
                tracing::warn!(incident = %incident.id(), "not delivering a card without its host");
                return None;
            }
            Err(err) => {
                tracing::warn!(%err, incident = %incident.id(), "could not load the card host");
                return None;
            }
        };
        let latest = self
            .store
            .list_for_incident(incident.id(), 1)
            .await
            .ok()
            .and_then(|signals| signals.into_iter().next());
        let report = self.latest_report(incident.id()).await;
        let actions = ActionRepo::list(
            &*self.store,
            &ActionFilter {
                incident: Some(incident.id()),
                ..ActionFilter::default()
            },
            &Page::first(Page::MAX_LIMIT),
        )
        .await
        .map(|page| page.items)
        .unwrap_or_default();
        Some(build_card(
            incident,
            report.as_ref(),
            &actions,
            &host.name,
            &CardLinks {
                web_url: self.config.web.public_url.as_ref().map(ToString::to_string),
                tokens,
            },
            latest.as_ref(),
        ))
    }

    async fn latest_report(&self, incident: IncidentId) -> Option<domovoi_core::TriageReport> {
        let entries = self.store.timeline(incident).await.ok()?;
        entries.into_iter().rev().find_map(|entry| {
            if !matches!(entry.kind.as_str(), "triage.report" | "triage.cached") {
                return None;
            }
            match serde_json::from_value::<TriageOutcome>(entry.detail).ok()? {
                TriageOutcome::Report(report) | TriageOutcome::Cached { report, .. } => {
                    Some(report)
                }
                _ => None,
            }
        })
    }

    pub(super) async fn recover(&self) {
        let mut page = Page::first(Page::MAX_LIMIT);
        loop {
            let got =
                match IncidentRepo::list(&*self.store, &IncidentFilter::default(), &page).await {
                    Ok(got) => got,
                    Err(err) => {
                        tracing::warn!(%err, "could not reload notification state after lag");
                        return;
                    }
                };
            for incident in got.items {
                let state = match NotifyStateRepo::get(&*self.store, incident.id()).await {
                    Ok(Some(state)) => state,
                    Ok(None) => NotifyState {
                        first_alert_at: Some(incident.opened_at()),
                        ..NotifyState::default()
                    },
                    Err(_) => continue,
                };
                let actions = notify::card_changed(&incident, &state);
                self.actions(incident, actions, state, BTreeMap::new())
                    .await;
            }
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => break,
            }
        }
    }

    pub(super) async fn send_all(&self, message: OutboundMessage) {
        let jobs = self.notifiers.iter().map(|notifier| async {
            match self.retry(|| notifier.send(&message)).await {
                Ok(()) => self.succeeded(notifier.id()).await,
                Err(err) => {
                    tracing::warn!(%err, notifier = %notifier.id(), "notification failed after retries");
                    self.failed(notifier.id()).await;
                }
            }
        });
        futures::future::join_all(jobs).await;
    }

    async fn retry<T, F>(&self, mut call: impl FnMut() -> F) -> Result<T, ChatError>
    where
        F: Future<Output = Result<T, ChatError>>,
    {
        let mut waits = VecDeque::from(BACKOFF);
        loop {
            match call().await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    let Some(seconds) = waits.pop_front() else {
                        return Err(err);
                    };
                    let now = self.clock.now();
                    self.clock
                        .sleep_until(
                            now.checked_add(SignedDuration::from_secs(seconds))
                                .unwrap_or(now),
                        )
                        .await;
                }
            }
        }
    }

    async fn failed(&self, gateway: &str) {
        metrics::counter!("domovoi_delivery_failures_total", "gateway" => gateway.to_owned())
            .increment(1);
        let count = {
            let mut counts = self
                .failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let count = counts.entry(gateway.to_owned()).or_default();
            *count = count.saturating_add(1);
            *count
        };
        let id = advisory_id(AdvisoryKind::DeliveryFailed, None, gateway);
        let created_at = self
            .store
            .list_open()
            .await
            .ok()
            .and_then(|open| open.into_iter().find(|a| a.id == id).map(|a| a.created_at))
            .unwrap_or_else(|| self.clock.now());
        let advisory = Advisory {
            id,
            kind: AdvisoryKind::DeliveryFailed,
            host: None,
            title: format!("Delivery to {gateway} is failing"),
            detail: format!(
                "{count} deliveries exhausted every retry. Check the gateway configuration and network."
            ),
            created_at,
            resolved_at: None,
        };
        if let Err(err) = self.store.upsert(&advisory).await {
            tracing::warn!(%err, %gateway, "could not record a delivery advisory");
        }
    }

    async fn succeeded(&self, gateway: &str) {
        let id = advisory_id(AdvisoryKind::DeliveryFailed, None, gateway);
        match self.store.resolve(id, self.clock.now()).await {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(err) => tracing::warn!(%err, %gateway, "could not clear a delivery advisory"),
        }
    }
}
