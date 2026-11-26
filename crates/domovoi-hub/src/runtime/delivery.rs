//! Chat delivery, reference persistence and bounded retries.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    AdvisoryRepo, ChatError, ChatGateway, Clock, HostRepo, IncidentRepo, MessageRef, Notifier,
    NotifyStateRepo, OutboundMessage, SignalRepo, StoreError,
};
use domovoi_core::{
    Advisory, AdvisoryKind, Card, ChatRefs, Incident, IncidentId, NotifyState, TriageOutcome,
};
use domovoi_engine::advisories::advisory_id;
use domovoi_engine::cards::{CardLinks, MAX_PLANS, build_card};
use domovoi_engine::events::{DomainEvent, EventKind};
use domovoi_engine::notify::{self, NotifyAction, NotifySettings};
use domovoi_engine::operations::waiting_steps;
use domovoi_store::Store;
use jiff::SignedDuration;

mod state;

use state::{Delivered, Working};

const BACKOFF: [i64; 5] = [1, 2, 4, 8, 16];

/// Counts a notification state read or write that failed, by `op`.
pub(super) fn state_error(op: &'static str) {
    metrics::counter!("domovoi_notify_state_errors_total", "op" => op).increment(1);
}

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
        // A state that cannot be read is not the same as one that was never written: a
        // default state has no card reference, so planning from it would post a second card.
        let base = match self.state(id).await {
            Ok(state) => state,
            Err(err) => {
                tracing::error!(
                    %err,
                    %id,
                    "could not read notification state; this event is not delivered"
                );
                state_error("read");
                return;
            }
        };
        let plan = notify::plan(&event, &base, &self.settings, self.clock.now());
        let tokens = match event.kind {
            EventKind::TriageCompleted { tokens, .. } => tokens,
            _ => BTreeMap::new(),
        };
        let delivered = self
            .actions(
                event.incident,
                plan.actions,
                Working {
                    base: base.clone(),
                    current: plan.state,
                },
                tokens,
            )
            .await;
        self.save_after(id, &base, delivered).await;
    }

    /// Delivers a tick's actions for one incident and saves the planned state only when every
    /// one of them was confirmed; otherwise only the references that were. Returns whether
    /// they all were.
    pub(super) async fn tick_actions(
        &self,
        incident: Incident,
        before: NotifyState,
        after: NotifyState,
        actions: Vec<NotifyAction>,
    ) -> bool {
        let id = incident.id();
        let delivered = self
            .actions(
                incident,
                actions,
                Working {
                    base: before.clone(),
                    current: after,
                },
                BTreeMap::new(),
            )
            .await;
        let ok = delivered.ok;
        self.save_after(id, &before, delivered).await;
        ok
    }

    async fn actions(
        &self,
        incident: Incident,
        actions: Vec<NotifyAction>,
        working: Working,
        tokens: BTreeMap<domovoi_core::ActionId, String>,
    ) -> Delivered {
        if actions.is_empty() || self.gateways.is_empty() {
            // Nothing to confirm: the plan's state is the truth, as with no chat configured.
            return Delivered {
                ok: true,
                state: working.current,
            };
        }
        let Some(card) = self.card(&incident, tokens).await else {
            return Delivered {
                ok: false,
                state: working.base,
            };
        };
        let working = Arc::new(tokio::sync::Mutex::new(working));
        let jobs = self.gateways.iter().map(|gateway| {
            let working = working.clone();
            let actions = actions.clone();
            let card = card.clone();
            let incident = incident.clone();
            async move {
                let mut ok = true;
                for action in actions {
                    // Every action still runs; one failure only marks the pass unconfirmed.
                    ok &= self
                        .perform(gateway, &incident, &card, &working, action)
                        .await;
                }
                ok
            }
        });
        let confirmed = futures::future::join_all(jobs)
            .await
            .into_iter()
            .all(|ok| ok);
        let state = working.lock().await.current.clone();
        Delivered {
            ok: confirmed,
            state,
        }
    }

    async fn perform(
        &self,
        gateway: &Arc<dyn ChatGateway>,
        incident: &Incident,
        card: &Card,
        state: &Arc<tokio::sync::Mutex<Working>>,
        action: NotifyAction,
    ) -> bool {
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
            Ok(()) => {
                self.succeeded(&platform).await;
                true
            }
            Err(err) => {
                tracing::warn!(%err, gateway = %platform, incident = %incident.id(), "delivery failed after retries");
                self.failed(&platform).await;
                false
            }
        }
    }

    async fn post(
        &self,
        gateway: &Arc<dyn ChatGateway>,
        incident: &Incident,
        card: &Card,
        state: &Arc<tokio::sync::Mutex<Working>>,
    ) -> Result<MessageRef, ChatError> {
        if let Some(message) = self.message(state, gateway.platform()).await {
            return Ok(message);
        }
        let message = self.retry(|| gateway.post_card(card)).await?;
        {
            let mut current = state.lock().await;
            current.current.messages.insert(
                gateway.platform().to_owned(),
                ChatRefs {
                    message: message.clone(),
                    thread: None,
                },
            );
            self.save_refs(incident.id(), &current).await;
        }
        let thread = self
            .retry(|| gateway.open_thread(&message, incident.title()))
            .await?;
        let mut current = state.lock().await;
        if let Some(refs) = current.current.messages.get_mut(gateway.platform()) {
            refs.thread = Some(thread);
        }
        self.save_refs(incident.id(), &current).await;
        Ok(message)
    }

    async fn update(
        &self,
        gateway: &Arc<dyn ChatGateway>,
        incident: &Incident,
        card: &Card,
        state: &Arc<tokio::sync::Mutex<Working>>,
    ) -> Result<(), ChatError> {
        let Some(message) = self.message(state, gateway.platform()).await else {
            return self.post(gateway, incident, card, state).await.map(|_| ());
        };
        match self.retry(|| gateway.update_card(&message, card)).await {
            Ok(()) => Ok(()),
            Err(ChatError::NotFound) => {
                state
                    .lock()
                    .await
                    .current
                    .messages
                    .remove(gateway.platform());
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
        state: &Arc<tokio::sync::Mutex<Working>>,
        text: &str,
    ) -> Result<(), ChatError> {
        let message = self.post(gateway, incident, card, state).await?;
        let existing = state
            .lock()
            .await
            .current
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
                if let Some(refs) = current.current.messages.get_mut(gateway.platform()) {
                    refs.thread = Some(thread.clone());
                }
                self.save_refs(incident.id(), &current).await;
                thread
            }
        };
        self.retry(|| gateway.reply(&thread, text)).await
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
        let steps = waiting_steps(&*self.store, incident, &host.name, MAX_PLANS)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(%err, "could not list the steps waiting for approval");
                Vec::new()
            });
        Some(build_card(
            incident,
            report.as_ref(),
            &[],
            &host.name,
            &CardLinks {
                web_url: self.config.web.public_url.as_ref().map(ToString::to_string),
                tokens,
                steps,
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

    /// Brings `incident`'s card up to date after missed events: an existing card is updated,
    /// and one whose opening event was missed is posted.
    pub(super) async fn recover(&self, incident: Incident) {
        let state = match NotifyStateRepo::get(&*self.store, incident.id()).await {
            Ok(Some(state)) => state,
            Ok(None) => NotifyState {
                first_alert_at: Some(incident.opened_at()),
                ..NotifyState::default()
            },
            Err(err) => {
                tracing::warn!(%err, incident = %incident.id(), "could not reload notification state");
                return;
            }
        };
        let actions = notify::card_changed(&incident, &state);
        let id = incident.id();
        let delivered = self
            .actions(
                incident,
                actions,
                Working {
                    base: state.clone(),
                    current: state.clone(),
                },
                BTreeMap::new(),
            )
            .await;
        // Recovery never advances the policy fields: only the references it confirmed change.
        self.save_after(id, &state, delivered).await;
    }

    /// Whether any one-way notifier is configured.
    pub(super) fn has_notifiers(&self) -> bool {
        !self.notifiers.is_empty()
    }

    /// Whether a notifier takes the quiet-hours summary.
    pub(super) fn can_replace_held_cards(&self) -> bool {
        self.notifiers.iter().any(|n| n.replaces_held_cards())
    }

    /// Sends `message` through every notifier and returns how many accepted it.
    pub(super) async fn send_all(&self, message: OutboundMessage) -> usize {
        let jobs = self.notifiers.iter().map(|notifier| async {
            match self.retry(|| notifier.send(&message)).await {
                Ok(()) => {
                    self.succeeded(notifier.id()).await;
                    true
                }
                Err(err) => {
                    tracing::warn!(%err, notifier = %notifier.id(), "notification failed after retries");
                    self.failed(notifier.id()).await;
                    false
                }
            }
        });
        futures::future::join_all(jobs)
            .await
            .into_iter()
            .filter(|sent| *sent)
            .count()
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
