//! The manager task: load state, decide, write, then publish.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use domovoi_core::ports::{
    AckRepo, Clock, CommitError, IncidentRepo, IncidentTimelineEntry, Redactor, StoreError,
    UnitOfWork, VersionedRead,
};
use domovoi_core::{HostId, Incident, IncidentId, IncidentStatus};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::command::{Command, CommandError};
use super::commit::{Versions, WRITE_CONFLICTS_METRIC, build_transaction, record_versions};
use super::decide::{Decision, State, Write, decide};
use super::handle::{COMMAND_QUEUE, Envelope, ManagerHandle};
use super::redact::redact_command;
use crate::events::{DomainEvent, EventKind};
use crate::policy::EnginePolicy;
use crate::telemetry;

/// The flap counter's metric name.
pub const FLAPS_METRIC: &str = "domovoi_incident_flaps_total";

/// What the incident manager needs.
#[derive(Clone)]
pub struct ManagerDeps {
    /// Incidents and their timelines; only the manager writes them.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Commits one command's writes as one transaction.
    pub uow: Arc<dyn UnitOfWork>,
    /// Reads an incident and the version its next write must name.
    pub versions: Arc<dyn VersionedRead>,
    /// Ack rules and maintenance windows.
    pub acks: Arc<dyn AckRepo>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Cleans signal summaries and text evidence before they are stored (INV-5).
    pub redactor: Arc<dyn Redactor>,
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
    versions: Versions,
    flaps: Arc<AtomicU64>,
    write_conflicts: Arc<AtomicU64>,
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
        let write_conflicts = Arc::new(AtomicU64::new(0));
        let handle = ManagerHandle {
            tx: tx.clone(),
            flaps: flaps.clone(),
            write_conflicts: write_conflicts.clone(),
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
            versions: Versions::default(),
            flaps,
            write_conflicts,
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
            self.process(envelope).await;
        }
        while let Ok(envelope) = rx.try_recv() {
            self.process(envelope).await;
        }
    }

    async fn process(&mut self, envelope: Envelope) {
        let Envelope { command, reply } = envelope;
        let command = redact_command(self.deps.redactor.as_ref(), command);
        let result = self.handle(&command).await;
        if let Err(err) = &result {
            tracing::warn!(%err, "an incident command failed");
        }
        if let Some(reply) = reply {
            let _ = reply.send(result);
        }
    }

    /// Replaces the cache of active incidents and their versions with what the store holds.
    async fn reload(&mut self) -> Result<(), StoreError> {
        let loaded = self.deps.versions.active_incidents().await?;
        self.active = Vec::with_capacity(loaded.len());
        self.versions.clear();
        for incident in loaded {
            self.versions.set(incident.value.id(), incident.version);
            self.active.push(incident.value);
        }
        Ok(())
    }

    async fn handle(&mut self, command: &Command) -> Result<(), CommandError> {
        let now = self.deps.clock.now();
        let mut attempt = 0_u8;
        loop {
            attempt += 1;
            let (state, loaded) = self.state_for(command, now).await.map_err(store)?;
            let decision = decide(&state, command, &self.deps.policy, now)?;
            let transaction = match build_transaction(&decision.writes, &self.versions, &loaded) {
                Ok(transaction) => transaction,
                Err(err) => {
                    tracing::warn!(
                        %err,
                        "an incident write had no known version; reloading and deciding again",
                    );
                    self.count_conflict();
                    self.reload_or_log().await;
                    if attempt == 1 {
                        continue;
                    }
                    return Err(err);
                }
            };
            match self.deps.uow.commit(transaction).await {
                Ok(receipt) => {
                    for write in &decision.writes {
                        self.remember(write);
                    }
                    record_versions(&mut self.versions, &self.active, &decision.writes, &receipt);
                    self.finish(command, decision);
                    return Ok(());
                }
                Err(CommitError::Conflict { aggregate, .. }) => {
                    tracing::warn!(
                        %aggregate,
                        "an incident write conflicted; reloading and deciding again",
                    );
                    self.count_conflict();
                    self.reload_or_log().await;
                    if attempt == 1 {
                        continue;
                    }
                    return Err(CommandError::Store(
                        "the incident changed twice while it was being updated".to_owned(),
                    ));
                }
                Err(err) => {
                    tracing::error!(%err, "an incident write failed; nothing is published");
                    self.reload_or_log().await;
                    return Err(CommandError::Store(err.to_string()));
                }
            }
        }
    }

    /// Counts one version conflict in the metric and in the handle.
    fn count_conflict(&self) {
        self.write_conflicts.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(WRITE_CONFLICTS_METRIC).increment(1);
    }

    /// Reloads after a failed command, logging rather than replacing its error.
    async fn reload_or_log(&mut self) {
        if let Err(err) = self.reload().await {
            tracing::error!(%err, "could not reload active incidents");
        }
    }

    /// Counts the flapping, metrics and events of a decision that was committed.
    fn finish(&mut self, command: &Command, decision: Decision) {
        if decision.flaps > 0 {
            self.flaps
                .fetch_add(u64::from(decision.flaps), Ordering::Relaxed);
            metrics::counter!(FLAPS_METRIC).increment(u64::from(decision.flaps));
        }
        if let Command::Ingest(signals) = command {
            for signal in signals {
                let source = telemetry::source_label(signal.source());
                metrics::counter!(telemetry::SIGNALS_TOTAL, "source" => source).increment(1);
            }
        }
        telemetry::record_active(&self.active);
        for event in decision.events {
            if let EventKind::TriageCompleted { outcome, .. } = &event.kind {
                let outcome = telemetry::outcome_label(outcome);
                metrics::counter!(telemetry::TRIAGE_RUNS_TOTAL, "outcome" => outcome).increment(1);
            }
            // No subscribers is fine: the event is already stored.
            let _ = self.deps.bus.send(event);
        }
    }

    /// Loads what deciding `command` needs beyond the cache, plus the versions of incidents it
    /// may write that are not already in the version cache.
    async fn state_for(
        &mut self,
        command: &Command,
        now: jiff::Timestamp,
    ) -> Result<(State, BTreeMap<IncidentId, u64>), StoreError> {
        let mut hosts: BTreeSet<HostId> = BTreeSet::new();
        let mut resolved = BTreeMap::new();
        let mut acks = Vec::new();
        let mut triaged = None;
        let mut timeline: Vec<IncidentTimelineEntry> = Vec::new();
        let mut loaded: BTreeMap<IncidentId, u64> = BTreeMap::new();
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
                    // A cache miss: the store may know an incident the cache does not. Read it
                    // again through the versioned port so body and version come from one read.
                    if let Some(found) = self
                        .deps
                        .incidents
                        .find_active_by_fingerprint(&fingerprint)
                        .await?
                    {
                        if let Some(read) = self.deps.versions.incident(found.id()).await?
                            && read.value.status() != IncidentStatus::Resolved
                        {
                            self.versions.set(read.value.id(), read.version);
                            self.active.push(read.value);
                        }
                        continue;
                    }
                    if let Some(found) = self
                        .deps
                        .incidents
                        .find_recent_resolved_by_fingerprint(&fingerprint, since)
                        .await?
                        && let Some(read) = self.deps.versions.incident(found.id()).await?
                    {
                        loaded.insert(read.value.id(), read.version);
                        resolved.insert(fingerprint, read.value);
                    }
                }
                acks = self.deps.acks.list_rules().await?;
            }
            Command::Tick => hosts.extend(self.active.iter().map(Incident::host)),
            Command::Ack { .. }
            | Command::Mute { .. }
            | Command::Unmute { .. }
            | Command::Dismiss { .. }
            | Command::RecordToolUse { .. }
            | Command::ClearFollowUps { .. } => {}
            // A recording follow-up needs the live timeline so deciding can tell whether
            // a clear landed while the model was still answering.
            Command::RecordFollowUp { incident, .. } => {
                timeline = self.deps.incidents.timeline(*incident).await?;
            }
            Command::AttachTriage { incident, .. } | Command::SetVerdict { incident, .. } => {
                if !self.active.iter().any(|i| i.id() == *incident)
                    && let Some(read) = self.deps.versions.incident(*incident).await?
                {
                    loaded.insert(read.value.id(), read.version);
                    triaged = Some(read.value);
                }
            }
        }
        let mut windows = Vec::new();
        for host in hosts {
            windows.extend(self.deps.acks.windows_for(host).await?);
        }
        let state = State {
            active: self.active.clone(),
            resolved,
            acks,
            windows,
            triaged,
            timeline,
        };
        Ok((state, loaded))
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
