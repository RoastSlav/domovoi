//! Carrying approved actions to completion (ARCHITECTURE.md §5.6, §12, ADR 0007).
//!
//! Only [`ActionHandle::approve`] moves an action to `Approved`, and it takes a
//! [`HumanApproval`] by value, which only the hub's auth layer can construct (INV-1). Chat
//! approvals also need the action's single-use token. The executor then applies a loop
//! guard, re-validates the plan against the agent's current manifest (INV-4), dispatches,
//! and verifies by re-running the originating check. Every transition is audited and
//! published as an event.

mod approvals;
pub mod audit;
mod commit;
mod executor;
mod expire;
mod freshness;
mod host_wait;
mod outcome;
mod preflight;
mod recheck;
mod reconcile;
mod recover;
mod reject;
mod verify_wake;
mod writer;
mod writer_propose;

pub use reconcile::MAX_RECONCILE_PASSES;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{
    ActionRepo, AdvisoryRepo, AgentLink, AuditLog, Clock, EffectKind, EffectQueue, IncidentRepo,
    Redactor, UnitOfWork,
};
use domovoi_core::{ActionError, ActionId, ActionRequest, HostId, HumanApproval, PlanBinding};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::{Notify, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::effects::{EffectRunner, EffectRunnerDeps, EffectSettings};
use crate::events::{DomainEvent, EventKind};
use crate::manager::ManagerHandle;

/// Limits and waits for running actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSettings {
    /// Dispatches of one skill on one host per rolling hour; more are blocked.
    pub loop_guard_per_hour: u32,
    /// The longest an approved action waits for its host's agent before it is rejected.
    pub offline_wait: SignedDuration,
    /// How often to look for the agent while waiting.
    pub presence_poll: SignedDuration,
    /// The wait before re-checking a write skill that declares no `verify`.
    pub verify_default_delay: SignedDuration,
    /// Added to the skill's own timeout for the dispatch deadline.
    pub dispatch_grace: SignedDuration,
    /// How long reconciliation waits before asking an agent again about a run that is
    /// still running.
    pub reconcile_interval: SignedDuration,
    /// How long one `action_status` request may take before its batch is skipped.
    pub reconcile_query_timeout: SignedDuration,
}

impl Default for ActionSettings {
    fn default() -> Self {
        Self {
            loop_guard_per_hour: 3,
            offline_wait: SignedDuration::from_mins(30),
            presence_poll: SignedDuration::from_secs(5),
            verify_default_delay: SignedDuration::from_secs(30),
            dispatch_grace: SignedDuration::from_secs(30),
            reconcile_interval: SignedDuration::from_secs(30),
            reconcile_query_timeout: SignedDuration::from_secs(10),
        }
    }
}

/// What the action services need. Everything is a port; the engine does no I/O.
#[derive(Clone)]
pub struct ActionDeps {
    /// Stored actions.
    pub actions: Arc<dyn ActionRepo>,
    /// Incidents, read only, for events and the check to verify.
    pub incidents: Arc<dyn IncidentRepo>,
    /// The append-only audit log, for reading back what past transitions recorded.
    pub audit: Arc<dyn AuditLog>,
    /// One atomic commit for an action write, its audit entry and its journal entry.
    pub uow: Arc<dyn UnitOfWork>,
    /// Claims and recovers durable effects.
    pub effects: Arc<dyn EffectQueue>,
    /// Where a dead effect is reported.
    pub advisories: Arc<dyn AdvisoryRepo>,
    /// Skill catalogues, dispatch and re-checks.
    pub link: Arc<dyn AgentLink>,
    /// Runs on skill output before it is stored.
    pub redactor: Arc<dyn Redactor>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// The incident manager, for the `Fixed` verdict.
    pub manager: ManagerHandle,
    /// The event bus: `ActionApproved` comes from it, every change goes to it.
    pub bus: broadcast::Sender<DomainEvent>,
    /// Stops the executor.
    pub shutdown: CancellationToken,
}

/// Why an approval or denial was refused. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalError {
    /// No action has that id.
    #[error("no such action {0}")]
    NotFound(ActionId),
    /// A chat approval without the action's token, with a wrong one, or with one already
    /// used.
    #[error("the approval link is not valid")]
    BadToken,
    /// The plan hash in the approve request is not the action's plan hash: the card the
    /// approver saw is stale or does not describe this action. Nothing changed.
    #[error("the plan changed since it was shown; reload and review it again")]
    PlanChanged,
    /// The action cannot make that change: wrong status, expired, or destructive without
    /// a typed confirmation on the web.
    #[error("{0}")]
    Refused(ActionError),
    /// Another runner holds the effect this write was fenced by: the lease expired and the
    /// effect was reclaimed. Nothing changed.
    #[error("the effect was claimed by someone else")]
    StaleEffect,
    /// The store failed.
    #[error("store error: {0}")]
    Store(String),
    /// The service that applies decisions is closing, so no decision can be made
    /// now. This is not a fault in the request: trying again after a moment works.
    #[error("the action service is shutting down; try again")]
    Unavailable,
}

/// Run dispatch times per host and skill, for the loop guard.
type Dispatches = BTreeMap<(HostId, String), VecDeque<Timestamp>>;

/// What a committed action write announces on the event bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Publish {
    /// Publish `ActionChanged` for the action.
    ActionChanged,
    /// Announce nothing: whoever queued the write announces it another way.
    Nothing,
}

pub(crate) struct Shared {
    pub(crate) deps: ActionDeps,
    pub(crate) settings: ActionSettings,
    /// Run dispatch times per host and skill, for the loop guard. Reloaded at start
    /// from the audit log (recover.rs), so a hub that restarted keeps the guard for
    /// the hour it has now.
    pub(crate) dispatches: Mutex<Dispatches>,
    /// Actions being run right now, so one is never run twice at once.
    pub(crate) running: Mutex<BTreeSet<ActionId>>,
    /// One gate per host, held from just before a dispatch is committed until the request
    /// has been handed to the agent's outgoing queue, and by reconciliation while it reads
    /// the host's pending rows. That is what keeps a committed-but-unsent run out of a
    /// reconciliation snapshot, so it can never be mistaken for a run the agent never got.
    pub(crate) dispatch_gates: Mutex<BTreeMap<HostId, Arc<tokio::sync::Mutex<()>>>>,
    /// One gate per host around a whole reconciliation pass, so a hello-triggered pass and
    /// one asked for by the executor cannot poll the same host twice at once.
    pub(crate) reconcile_passes: Mutex<BTreeMap<HostId, Arc<tokio::sync::Mutex<()>>>>,
    /// Hosts whose reconciliation was asked for while no pass was running for them.
    pub(crate) reconcile_pending: Mutex<BTreeSet<HostId>>,
    /// Hosts that asked for a pass while one was already running for them. The running pass
    /// cannot see rows dispatched after its snapshot, so it queues one more pass when it
    /// releases its gate instead of dropping the request.
    pub(crate) reconcile_again: Mutex<BTreeSet<HostId>>,
    /// Bounds how many hosts reconcile at once, so a burst of triggers starts a bounded
    /// number of passes and one slow host cannot keep every other host waiting.
    pub(crate) reconcile_slots: Arc<tokio::sync::Semaphore>,
    /// How many hosts the reconciliation scheduler has taken from its waiting set. Test
    /// support: it lets a test wait until a trigger has actually been considered instead of
    /// guessing with sleeps.
    pub(crate) reconcile_attempts: std::sync::atomic::AtomicU64,
    /// Wakes the reconciliation worker.
    pub(crate) reconcile_nudge: Arc<Notify>,
    /// Outcomes that could not be queued on the writer yet; one worker hands them over.
    pub(crate) deferred: mpsc::UnboundedSender<writer::WriteCommand>,
    /// The queue into the writer task. The executor hands every transition to that
    /// one writer (ADR 0019) rather than writing action state itself.
    pub(crate) writer: writer::WriterHandle,
    /// Limits the runs started from the store at once. Shared across catch-ups, so a
    /// reload after the event bus fell behind does not open a second set of runs.
    pub(crate) catch_up_slots: Arc<tokio::sync::Semaphore>,
    /// Wakes the effect runner as soon as a writer commits an effect that is due, so a
    /// verification does not wait for the runner's next poll.
    pub(crate) effects_nudge: Arc<Notify>,
}

impl Shared {
    /// The host's dispatch gate; see [`Shared::dispatch_gates`]. Held while a dispatch is
    /// committed and handed to the agent, and while reconciliation reads the pending rows.
    pub(crate) async fn dispatch_gate(&self, host: HostId) -> tokio::sync::OwnedMutexGuard<()> {
        let gate = {
            let mut gates = self
                .dispatch_gates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            gates
                .entry(host)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        gate.lock_owned().await
    }

    /// Asks for a reconciliation of `host` soon. Repeated asks for a host that is already
    /// waiting collapse into one pass; the worker keeps at most one entry per host, so this
    /// cannot grow into a loop of its own.
    pub(crate) fn schedule_reconciliation(&self, host: HostId) {
        self.reconcile_pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(host);
        self.reconcile_nudge.notify_one();
    }

    /// Publishes `kind` for the incident `action` belongs to.
    pub(crate) async fn publish(&self, action: &ActionRequest, kind: EventKind) {
        match self.deps.incidents.get(action.incident()).await {
            Ok(Some(incident)) => {
                let _ = self.deps.bus.send(DomainEvent::new(kind, &incident, true));
            }
            Ok(None) => tracing::warn!(action = %action.id(), "an action's incident is gone"),
            Err(err) => tracing::warn!(%err, "could not load an action's incident"),
        }
    }
}

/// Reaches the action services. Cheap to clone.
#[derive(Clone)]
pub struct ActionHandle {
    shared: Arc<Shared>,
    writer: writer::WriterHandle,
}

impl std::fmt::Debug for ActionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionHandle")
            .field("settings", &self.shared.settings)
            .finish_non_exhaustive()
    }
}

/// The approval service and the executor.
#[derive(Debug)]
pub struct ActionService;

impl ActionService {
    /// Starts one task running both halves of the action service: the writer
    /// loop, which applies every decision one at a time (ADR 0019), and the
    /// executor, which runs approved actions until `deps.shutdown`. They are
    /// joined in one handle so whoever waits for the service also waits for the
    /// decisions still queued when shutdown starts.
    #[must_use]
    pub fn spawn(deps: ActionDeps, settings: ActionSettings) -> (ActionHandle, JoinHandle<()>) {
        let events = deps.bus.subscribe();
        let (writer, incoming) = writer::queue();
        let (deferred, deferred_incoming) = mpsc::unbounded_channel();
        let effects_nudge = Arc::new(Notify::new());
        let reconcile_nudge = Arc::new(Notify::new());
        let shared = Arc::new(Shared {
            deps,
            settings,
            dispatches: Mutex::new(BTreeMap::new()),
            running: Mutex::new(BTreeSet::new()),
            dispatch_gates: Mutex::new(BTreeMap::new()),
            reconcile_passes: Mutex::new(BTreeMap::new()),
            reconcile_pending: Mutex::new(BTreeSet::new()),
            reconcile_again: Mutex::new(BTreeSet::new()),
            reconcile_slots: Arc::new(tokio::sync::Semaphore::new(reconcile::MAX_RECONCILE_PASSES)),
            reconcile_attempts: std::sync::atomic::AtomicU64::new(0),
            reconcile_nudge: reconcile_nudge.clone(),
            deferred,
            writer: writer.clone(),
            catch_up_slots: Arc::new(tokio::sync::Semaphore::new(executor::MAX_CATCH_UP_RUNS)),
            effects_nudge: effects_nudge.clone(),
        });
        let mut runner = EffectRunner::new(
            EffectRunnerDeps {
                queue: shared.deps.effects.clone(),
                uow: shared.deps.uow.clone(),
                clock: shared.deps.clock.clone(),
                advisories: shared.deps.advisories.clone(),
                nudge: effects_nudge,
                shutdown: shared.deps.shutdown.clone(),
            },
            EffectSettings::default(),
        );
        runner.register(
            EffectKind::Wake,
            Arc::new(verify_wake::VerifyWake::new(shared.clone())),
        );
        let writer_shared = shared.clone();
        let deferred_shared = shared.clone();
        let executor_shared = shared.clone();
        let reconcile_shared = shared.clone();
        let task = tokio::spawn(async {
            // The loop guard counts runs in a rolling hour, and the runs of the session
            // that just stopped were in its memory, so they are read back from the audit
            // log before either half can start a run that would be checked against an
            // empty guard.
            let reloaded = match recover::dispatch_history(&executor_shared).await {
                Ok(count) => count,
                Err(err) => {
                    tracing::error!(
                        %err,
                        "the loop guard could not reload its last hour; it starts with none",
                    );
                    0
                }
            };
            if reloaded > 0 {
                tracing::info!(
                    count = reloaded,
                    "the loop guard reloaded dispatches the previous hub session made",
                );
            }
            // Startup order: the loop guard is reloaded before either half can start a
            // run, because the executor's catch-up reads the store for approvals as soon
            // as it starts. The hub starts its agent server before this service, so each
            // host's availability is already answerable when the executor asks. A
            // dispatched run left by the previous session is not touched here: its outcome
            // is reconciled when that host's agent authenticates. The effect runner starts
            // with them: it returns what a previous session left claimed to pending before
            // it claims anything, so a verification due across a restart still runs.
            let _ = tokio::join!(
                writer::run(writer_shared, incoming),
                writer::feed_deferred(deferred_shared, deferred_incoming),
                executor::serve(executor_shared, events),
                reconcile::serve(reconcile_shared),
                runner.run(),
            );
        });
        (ActionHandle { shared, writer }, task)
    }
}

impl ActionHandle {
    /// Whether the action service's writer is still taking decisions. A closed queue
    /// means the service ended, so approvals queued now would be refused.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.writer.tx.is_closed()
    }

    /// Test support: how many hosts the reconciliation scheduler has taken from its waiting
    /// set. Not part of the public contract; it exists so a test can wait until a trigger has
    /// actually been considered instead of guessing with sleeps.
    #[doc(hidden)]
    #[must_use]
    pub fn reconcile_attempts(&self) -> u64 {
        self.shared
            .reconcile_attempts
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Approves action `id` on behalf of the human in `approval`. A chat approval needs
    /// `chat_token`, which works once. `binding` must name the action's stored v1 plan hash,
    /// so an approval covers the plan the card showed. This is the only way to reach
    /// `Approved`; the decision is queued for the writer task, so it cannot race another
    /// one.
    pub async fn approve(
        &self,
        id: ActionId,
        approval: HumanApproval,
        chat_token: Option<&str>,
        binding: PlanBinding,
    ) -> Result<ActionRequest, ApprovalError> {
        let (reply, answer) = oneshot::channel();
        let command = writer::WriteCommand::Approve {
            id,
            approval,
            chat_token: chat_token.map(str::to_owned),
            binding,
            reply,
        };
        if self.writer.tx.send(command).await.is_err() {
            return Err(ApprovalError::Unavailable);
        }
        match answer.await {
            Ok(result) => result,
            Err(_) => Err(ApprovalError::Unavailable),
        }
    }

    /// Denies action `id` on behalf of the human in `approval`; queued for the
    /// writer task like [`ActionHandle::approve`].
    pub async fn deny(
        &self,
        id: ActionId,
        approval: HumanApproval,
    ) -> Result<ActionRequest, ApprovalError> {
        let (reply, answer) = oneshot::channel();
        let command = writer::WriteCommand::Deny {
            id,
            approval,
            reply,
        };
        if self.writer.tx.send(command).await.is_err() {
            return Err(ApprovalError::Unavailable);
        }
        match answer.await {
            Ok(result) => result,
            Err(_) => Err(ApprovalError::Unavailable),
        }
    }

    /// Stores a newly proposed action with its audit and journal entries. Refused when the
    /// action is not in the `proposed` status, or when its id is already stored. The
    /// write is queued for the writer task, so triage never writes action state itself.
    pub async fn propose(&self, action: ActionRequest) -> Result<ActionRequest, ApprovalError> {
        let (reply, answer) = oneshot::channel();
        let command = writer::WriteCommand::Propose {
            action: Box::new(action),
            reply,
        };
        if self.writer.tx.send(command).await.is_err() {
            return Err(ApprovalError::Unavailable);
        }
        match answer.await {
            Ok(result) => result,
            Err(_) => Err(ApprovalError::Unavailable),
        }
    }

    /// Replaces the chat approval token hash of a still-proposed action. The previous
    /// token stops working. Refused with `ChangedMeanwhile` when the action is no longer
    /// `proposed`, and `NotFound` when it does not exist.
    pub async fn rotate_chat_token(
        &self,
        id: ActionId,
        token_hash: [u8; 32],
    ) -> Result<ActionRequest, ApprovalError> {
        let (reply, answer) = oneshot::channel();
        let command = writer::WriteCommand::RotateChatToken {
            id,
            token_hash,
            reply,
        };
        if self.writer.tx.send(command).await.is_err() {
            return Err(ApprovalError::Unavailable);
        }
        match answer.await {
            Ok(result) => result,
            Err(_) => Err(ApprovalError::Unavailable),
        }
    }

    /// Expires every proposed action whose window has closed, and every approved action that
    /// closed before it was dispatched as never run, and returns how many. The scheduler
    /// calls this periodically.
    pub async fn expire_due(&self) -> Result<usize, ApprovalError> {
        let (reply, answer) = oneshot::channel();
        if self
            .writer
            .tx
            .send(writer::WriteCommand::ExpireDue { reply })
            .await
            .is_err()
        {
            return Err(ApprovalError::Unavailable);
        }
        match answer.await {
            Ok(result) => result,
            Err(_) => Err(ApprovalError::Unavailable),
        }
    }
}

#[async_trait::async_trait]
impl domovoi_core::ports::HostReconciler for ActionHandle {
    async fn reconcile_host(&self, host: domovoi_core::HostId) {
        reconcile::host(&self.shared, host).await;
    }
}
