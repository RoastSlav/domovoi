//! The manager task: load state, decide, write, then publish.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use domovoi_core::ports::{
    AckRepo, Clock, IncidentFilter, IncidentRepo, Page, SignalRepo, StoreError,
};
use domovoi_core::{HostId, Incident, IncidentStatus};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::command::{Command, CommandError};
use super::decide::{State, Write, decide};
use super::handle::{COMMAND_QUEUE, Envelope, ManagerHandle};
use crate::events::DomainEvent;
use crate::policy::EnginePolicy;

/// The flap counter's metric name.
pub const FLAPS_METRIC: &str = "domovoi_incident_flaps_total";

/// What the incident manager needs.
#[derive(Clone)]
pub struct ManagerDeps {
    /// Incidents and their timelines; only the manager writes them.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Signals.
    pub signals: Arc<dyn SignalRepo>,
    /// Ack rules and maintenance windows.
    pub acks: Arc<dyn AckRepo>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Confirmation, grouping and tick settings.
    pub policy: EnginePolicy,
    /// Where events go once stored.
    pub bus: broadcast::Sender<DomainEvent>,
    /// Stops the manager and its ticker.
    pub shutdown: CancellationToken,
}

/// The single writer for incident state.
pub struct IncidentManager {
    deps: ManagerDeps,
    active: Vec<Incident>,
    flaps: Arc<AtomicU64>,
}

fn store(err: StoreError) -> CommandError {
    CommandError::Store(err.to_string())
}

impl IncidentManager {
    /// Starts the manager and its ticker. The manager stops on shutdown or once every
    /// handle is dropped.
    #[must_use]
    pub fn spawn(deps: ManagerDeps) -> (ManagerHandle, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        let flaps = Arc::new(AtomicU64::new(0));
        let handle = ManagerHandle {
            tx: tx.clone(),
            flaps: flaps.clone(),
        };
        tokio::spawn(ticker(
            tx.downgrade(),
            deps.clock.clone(),
            deps.policy,
            deps.shutdown.clone(),
        ));
        drop(tx);
        let manager = Self {
            deps,
            active: Vec::new(),
            flaps,
        };
        let task = tokio::spawn(manager.run(rx));
        (handle, task)
    }

    async fn run(mut self, mut rx: mpsc::Receiver<Envelope>) {
        if let Err(err) = self.reload().await {
            tracing::error!(%err, "could not load active incidents; retrying on the next command");
        }
        let shutdown = self.deps.shutdown.clone();
        loop {
            let envelope = tokio::select! {
                () = shutdown.cancelled() => break,
                envelope = rx.recv() => match envelope {
                    Some(envelope) => envelope,
                    None => break,
                },
            };
            let result = self.handle(&envelope.command).await;
            if let Err(err) = &result {
                tracing::warn!(%err, "an incident command failed");
            }
            if let Some(reply) = envelope.reply {
                let _ = reply.send(result);
            }
        }
    }

    /// Replaces the cache of active incidents with what the store holds.
    async fn reload(&mut self) -> Result<(), StoreError> {
        let filter = IncidentFilter {
            active: Some(true),
            ..IncidentFilter::default()
        };
        let mut page = Page::first(Page::MAX_LIMIT);
        let mut active = Vec::new();
        loop {
            let got = self.deps.incidents.list(&filter, &page).await?;
            active.extend(got.items);
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => break,
            }
        }
        self.active = active;
        Ok(())
    }

    async fn handle(&mut self, command: &Command) -> Result<(), CommandError> {
        let now = self.deps.clock.now();
        let state = self.state_for(command, now).await.map_err(store)?;
        let decision = decide(&state, command, &self.deps.policy, now)?;
        for write in &decision.writes {
            if let Err(err) = self.write(write).await {
                tracing::error!(%err, "an incident write failed; nothing is published");
                if let Err(err) = self.reload().await {
                    tracing::error!(%err, "could not reload active incidents");
                }
                return Err(store(err));
            }
        }
        for write in &decision.writes {
            self.remember(write);
        }
        if decision.flaps > 0 {
            self.flaps
                .fetch_add(u64::from(decision.flaps), Ordering::Relaxed);
            metrics::counter!(FLAPS_METRIC).increment(u64::from(decision.flaps));
        }
        for event in decision.events {
            // No subscribers is fine: the event is already stored.
            let _ = self.deps.bus.send(event);
        }
        Ok(())
    }

    /// Loads what deciding `command` needs beyond the cache.
    async fn state_for(
        &mut self,
        command: &Command,
        now: jiff::Timestamp,
    ) -> Result<State, StoreError> {
        let mut hosts: BTreeSet<HostId> = BTreeSet::new();
        let mut resolved = BTreeMap::new();
        let mut acks = Vec::new();
        let mut triaged = None;
        match command {
            Command::Ingest(signals) => {
                let since = now
                    .checked_sub(self.deps.policy.incident.reopen_window)
                    .unwrap_or(now);
                for signal in signals {
                    hosts.insert(signal.host());
                    let fingerprint = signal.fingerprint();
                    if self.active.iter().any(|i| i.is_member(&fingerprint)) {
                        continue;
                    }
                    // A cache miss: the store may know an incident the cache does not.
                    if let Some(found) = self
                        .deps
                        .incidents
                        .find_active_by_fingerprint(&fingerprint)
                        .await?
                    {
                        self.active.push(found);
                        continue;
                    }
                    if let Some(found) = self
                        .deps
                        .incidents
                        .find_recent_resolved_by_fingerprint(&fingerprint, since)
                        .await?
                    {
                        resolved.insert(fingerprint, found);
                    }
                }
                acks = self.deps.acks.list_rules().await?;
            }
            Command::Tick => hosts.extend(self.active.iter().map(Incident::host)),
            Command::Ack { .. }
            | Command::Mute { .. }
            | Command::Unmute { .. }
            | Command::RecordFollowUp { .. } => {}
            Command::AttachTriage { incident, .. } => {
                if !self.active.iter().any(|i| i.id() == *incident) {
                    triaged = self.deps.incidents.get(*incident).await?;
                }
            }
        }
        let mut windows = Vec::new();
        for host in hosts {
            windows.extend(self.deps.acks.windows_for(host).await?);
        }
        Ok(State {
            active: self.active.clone(),
            resolved,
            acks,
            windows,
            triaged,
        })
    }

    async fn write(&self, write: &Write) -> Result<(), StoreError> {
        let incidents = &self.deps.incidents;
        match write {
            Write::InsertSignal(signal, incident) => {
                self.deps.signals.insert(signal, *incident).await
            }
            Write::InsertIncident(incident) => incidents.insert(incident).await,
            Write::UpdateIncident(incident) => incidents.update(incident).await,
            Write::Timeline(entry) => incidents.append_event(entry).await,
            Write::DiscardPending(id) => incidents.discard_pending(*id).await,
        }
    }

    /// Updates the cache after a write was stored.
    fn remember(&mut self, write: &Write) {
        match write {
            Write::InsertIncident(incident) | Write::UpdateIncident(incident) => {
                self.active.retain(|i| i.id() != incident.id());
                if incident.status() != IncidentStatus::Resolved {
                    self.active.push(incident.clone());
                }
            }
            Write::DiscardPending(id) => self.active.retain(|i| i.id() != *id),
            Write::InsertSignal(..) | Write::Timeline(_) => {}
        }
    }
}

/// Sends `Tick` every `tick_every` until shutdown or until the manager is gone.
async fn ticker(
    tx: mpsc::WeakSender<Envelope>,
    clock: Arc<dyn Clock>,
    policy: EnginePolicy,
    shutdown: CancellationToken,
) {
    let every = policy.tick_every.max(jiff::SignedDuration::from_secs(1));
    let mut next = clock.now().checked_add(every).unwrap_or(clock.now());
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = clock.sleep_until(next) => {}
        }
        let Some(tx) = tx.upgrade() else { return };
        let tick = Envelope {
            command: Command::Tick,
            reply: None,
        };
        // A full queue already has work queued; skip this tick.
        if let Err(mpsc::error::TrySendError::Closed(_)) = tx.try_send(tick) {
            return;
        }
        drop(tx);
        next = next.checked_add(every).unwrap_or(next);
        if next <= clock.now() {
            next = clock.now().checked_add(every).unwrap_or(next);
        }
    }
}
