//! The claim loop, its retries and its dead-lettering.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::EffectId;
use domovoi_core::ports::repos::AdvisoryRepo;
use domovoi_core::ports::{
    AggregateKind, ClaimToken, Clock, EffectDeath, EffectKind, EffectQueue, EffectRecord,
    EffectReschedule, Transaction, UnitOfWork,
};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::Notify;
use tokio::task::{Id as TaskId, JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use super::{EffectHandler, EffectOutcome, EffectSettings, backoff};
use crate::advisories::effect_dead;

/// What the runner needs. Everything is a port; the engine does no I/O.
#[derive(Clone)]
pub struct EffectRunnerDeps {
    /// Where due effects are claimed.
    pub queue: Arc<dyn EffectQueue>,
    /// Commits a retry or a dead letter.
    pub uow: Arc<dyn UnitOfWork>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Where a dead effect is reported.
    pub advisories: Arc<dyn AdvisoryRepo>,
    /// Wakes the runner before its next poll.
    pub nudge: Arc<Notify>,
    /// Stops the runner.
    pub shutdown: CancellationToken,
}

/// Claims due effects and runs their handlers until shutdown.
pub struct EffectRunner {
    deps: EffectRunnerDeps,
    settings: EffectSettings,
    handlers: BTreeMap<EffectKind, Arc<dyn EffectHandler>>,
    runner_id: String,
    aggregates: Vec<AggregateKind>,
}

impl EffectRunner {
    /// A runner that claims under its own runner id and hands each kind to a handler.
    #[must_use]
    pub fn new(deps: EffectRunnerDeps, settings: EffectSettings) -> Self {
        Self {
            deps,
            settings,
            handlers: BTreeMap::new(),
            runner_id: format!("hub-{}", ClaimToken::new()),
            aggregates: AggregateKind::LEGACY.to_vec(),
        }
    }

    /// Limits the runner to effects of these aggregate kinds. A new runner claims only the
    /// legacy aggregates, so it never takes another service's rows.
    #[must_use]
    pub fn scoped(mut self, aggregates: Vec<AggregateKind>) -> Self {
        self.aggregates = aggregates;
        self
    }

    /// Registers the handler for `kind`, replacing any previous one.
    pub fn register(&mut self, kind: EffectKind, handler: Arc<dyn EffectHandler>) {
        self.handlers.insert(kind, handler);
    }

    /// Wakes the runner now, for an owner that has just committed a new effect.
    pub fn nudge(&self) {
        self.deps.nudge.notify_one();
    }

    /// Claims and runs due effects until shutdown. In-flight handlers are awaited on the way
    /// out; anything still claimed when the process stops is put back at the next start.
    pub async fn run(self) {
        if self.deps.shutdown.is_cancelled() {
            return;
        }
        match self
            .deps
            .queue
            .revert_claimed_at_start(&self.aggregates)
            .await
        {
            Ok(count) if count > 0 => {
                tracing::info!(
                    count,
                    "effects left claimed by the previous session were returned to pending",
                );
            }
            Ok(_) => {}
            Err(err) => tracing::error!(
                %err,
                "could not return claimed effects to pending; their leases will expire instead",
            ),
        }
        let kinds: Vec<EffectKind> = self.handlers.keys().copied().collect();
        if kinds.is_empty() {
            tracing::debug!("the effect runner has no handlers; it claims nothing");
            return;
        }
        let mut in_flight = JoinSet::new();
        // Which effect each running handler task holds, so a task that dies without a result
        // can be named in the log.
        let mut running = Running::default();
        while !self.deps.shutdown.is_cancelled() {
            while let Some(finished) = in_flight.try_join_next_with_id() {
                joined(finished, &mut running);
            }
            let now = self.deps.clock.now();
            // A stopping hub stops claiming at once rather than waiting out a store call;
            // anything already claimed is put back at the next start.
            let claimed = tokio::select! {
                () = self.deps.shutdown.cancelled() => break,
                claimed = self.deps.queue.claim_due(
                    &self.runner_id,
                    &kinds,
                    &self.aggregates,
                    now,
                    self.settings.lease,
                    self.settings.batch,
                ) => claimed,
            };
            let batch = match claimed {
                Ok(batch) => batch,
                Err(err) => {
                    tracing::warn!(%err, "could not claim due effects; trying again after the poll");
                    self.wait(self.settings.poll).await;
                    continue;
                }
            };
            // The claim pass spends its limit on selected candidates, whether it claimed them
            // or dead-lettered them, so both count towards a full batch.
            let processed = batch.claimed.len() + batch.dead.len();
            let full = processed >= usize::from(self.settings.batch);
            for record in batch.dead {
                let error = record
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "attempt limit reached".to_owned());
                self.raise(&record, &error, now).await;
            }
            for claimed in batch.claimed {
                let Some(handler) = EffectKind::parse(&claimed.record.kind)
                    .and_then(|kind| self.handlers.get(&kind))
                    .cloned()
                else {
                    tracing::warn!(
                        kind = %claimed.record.kind,
                        "no handler is registered for this effect kind; its lease will expire",
                    );
                    continue;
                };
                let deps = self.deps.clone();
                let record = claimed.record;
                let id = record.id;
                let task = in_flight.spawn(async move {
                    let outcome = handler.handle(&record, claimed.token).await;
                    apply(&deps, record, claimed.token, outcome).await;
                });
                running.start(task.id(), id);
            }
            if full {
                continue;
            }
            self.wait(self.settings.poll).await;
        }
        while let Some(finished) = in_flight.join_next_with_id().await {
            joined(finished, &mut running);
        }
    }

    /// Sleeps for `poll`, waking early on a nudge or shutdown.
    async fn wait(&self, poll: SignedDuration) {
        let now = self.deps.clock.now();
        let until = now.checked_add(poll).unwrap_or(now);
        tokio::select! {
            () = self.deps.shutdown.cancelled() => {}
            () = self.deps.nudge.notified() => {}
            () = self.deps.clock.sleep_until(until) => {}
        }
    }

    /// Reports a dead effect as an advisory. A failure here is logged: the effect stays
    /// dead either way, so nothing loops.
    async fn raise(&self, record: &EffectRecord, error: &str, now: Timestamp) {
        let advisory = effect_dead(record, error, now);
        if let Err(err) = self.deps.advisories.upsert(&advisory).await {
            tracing::warn!(%err, effect = %record.id, "could not raise the dead-effect advisory");
        }
    }
}

/// Which effect each running handler task holds.
///
/// Keyed by task, never by effect: a handler that outlives its lease leaves one generation in
/// flight while the runner reclaims the effect and starts another, so the same effect can have
/// two tasks at once. Finishing one must never drop the other one's identity.
#[derive(Default)]
pub(super) struct Running(BTreeMap<TaskId, EffectId>);

impl Running {
    /// Records that `task` is handling `effect`.
    pub(super) fn start(&mut self, task: TaskId, effect: EffectId) {
        self.0.insert(task, effect);
    }

    /// Forgets exactly this task and returns the effect it held, if it was tracked.
    pub(super) fn finish(&mut self, task: TaskId) -> Option<EffectId> {
        self.0.remove(&task)
    }

    /// How many task generations are still in flight.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.0.len()
    }
}

/// Consumes one finished handler task.
///
/// A handler that panicked, or that was cancelled, produced no outcome: its effect stays
/// claimed exactly as it was, and only the lease expiry or the next start may reclaim it.
/// The failure is logged at error level so it is not silently discarded, naming the effect
/// that this exact task held.
fn joined(result: Result<(TaskId, ()), JoinError>, running: &mut Running) {
    match result {
        Ok((task, ())) => {
            running.finish(task);
        }
        Err(err) => {
            let held = running.finish(err.id());
            if err.is_cancelled() {
                tracing::error!(
                    effect = ?held,
                    task = %err.id(),
                    "an effect handler task was cancelled without a result; its effect stays claimed",
                );
            } else {
                tracing::error!(
                    effect = ?held,
                    task = %err.id(),
                    "an effect handler failed without a result; its effect stays claimed until its lease expires",
                );
            }
        }
    }
}

/// Turns a handler's outcome into effect state, or nothing at all when it handled it.
async fn apply(
    deps: &EffectRunnerDeps,
    record: EffectRecord,
    token: ClaimToken,
    outcome: EffectOutcome,
) {
    match outcome {
        EffectOutcome::Handled => {}
        EffectOutcome::Dead { error } => dead(deps, &record, token, error).await,
        EffectOutcome::Retry { error } => {
            if record.attempt >= record.max_attempts {
                dead(deps, &record, token, format!("{error} (attempt limit)")).await;
                return;
            }
            let now = deps.clock.now();
            let not_before = now.checked_add(backoff(record.attempt)).unwrap_or(now);
            let tx = Transaction::new().reschedule(EffectReschedule {
                id: record.id,
                claim_token: token,
                not_before,
                error,
                at: now,
            });
            commit(deps, tx, &record).await;
        }
    }
}

/// Dead-letters a claimed effect and reports it.
async fn dead(deps: &EffectRunnerDeps, record: &EffectRecord, token: ClaimToken, error: String) {
    let now = deps.clock.now();
    let tx = Transaction::new().kill(EffectDeath {
        id: record.id,
        claim_token: token,
        error: error.clone(),
        at: now,
    });
    if commit(deps, tx, record).await {
        let advisory = effect_dead(record, &error, now);
        if let Err(err) = deps.advisories.upsert(&advisory).await {
            tracing::warn!(%err, effect = %record.id, "could not raise the dead-effect advisory");
        }
    }
}

/// Commits a retry or death, logging a stale claim (another runner holds the effect now) and
/// any store failure (the lease expires and the effect is claimed again).
async fn commit(deps: &EffectRunnerDeps, tx: Transaction, record: &EffectRecord) -> bool {
    match deps.uow.commit(tx).await {
        Ok(_) => true,
        Err(domovoi_core::ports::CommitError::StaleEffect { .. }) => {
            tracing::warn!(
                effect = %record.id,
                "stale lease holder discarded: another runner holds this effect now",
            );
            false
        }
        Err(err) => {
            tracing::error!(
                %err,
                effect = %record.id,
                "could not record an effect's outcome; its lease expires and it is claimed again",
            );
            false
        }
    }
}
