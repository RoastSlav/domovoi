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
mod executor;
mod recheck;
mod recover;
mod writer;
mod writer_propose;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{
    ActionGuard, ActionRepo, AgentLink, AggregateRef, AuditDraft, AuditLog, Clock, CommitError,
    IncidentRepo, JournalDraft, JournalKind, Redactor, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{ActionError, ActionId, ActionRequest, HostId, HumanApproval};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::events::{DomainEvent, EventKind};
use crate::manager::ManagerHandle;

/// Limits and waits for running actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSettings {
    /// Dispatches of one skill on one host per rolling hour; more are blocked.
    pub loop_guard_per_hour: u32,
    /// How long to wait for an offline agent before failing.
    pub offline_wait: SignedDuration,
    /// How often to look for the agent while waiting.
    pub presence_poll: SignedDuration,
    /// The wait before re-checking a write skill that declares no `verify`.
    pub verify_default_delay: SignedDuration,
    /// Added to the skill's own timeout for the dispatch deadline.
    pub dispatch_grace: SignedDuration,
}

impl Default for ActionSettings {
    fn default() -> Self {
        Self {
            loop_guard_per_hour: 3,
            offline_wait: SignedDuration::from_mins(5),
            presence_poll: SignedDuration::from_secs(5),
            verify_default_delay: SignedDuration::from_secs(30),
            dispatch_grace: SignedDuration::from_secs(30),
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
    /// The action cannot make that change: wrong status, expired, or destructive without
    /// a typed confirmation on the web.
    #[error("{0}")]
    Refused(ActionError),
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
    /// The queue into the writer task. The executor hands every transition to that
    /// one writer (ADR 0019) rather than writing action state itself.
    pub(crate) writer: writer::WriterHandle,
}

impl Shared {
    /// Commits one action write together with its audit and journal entries, then records
    /// the status-change metric (only when the status actually changed) and publishes the
    /// event. Nothing is recorded or published when the commit fails, so a store error can
    /// never leave an action in a new status without its audit and journal rows. The
    /// journal's `from` is the status the guard names; it is `null` for a creation.
    pub(crate) async fn commit_action(
        &self,
        action: &ActionRequest,
        guard: ActionGuard,
        audit: AuditDraft,
        now: Timestamp,
        extra: Option<(&str, &str)>,
        publish: Publish,
    ) -> Result<(), CommitError> {
        let (from, changed) = match &guard {
            ActionGuard::New => (None, true),
            ActionGuard::Status(status) => (
                Some(status.as_str()),
                status.as_str() != action.status().name(),
            ),
        };
        let mut payload = json!({
            "action": action.id().to_string(),
            "incident": action.incident().to_string(),
            "host": action.host().to_string(),
            "skill": action.skill().as_str(),
            "from": from,
            "to": action.status().name(),
            "plan_hash": action.plan_hash().to_string(),
        });
        if let (Value::Object(fields), Some((key, value))) = (&mut payload, extra) {
            fields.insert(key.to_owned(), json!(value));
        }
        let journal = JournalDraft {
            aggregate: AggregateRef::action(action.id()),
            kind: JournalKind::ActionTransition,
            payload_v: 1,
            payload,
            actor: audit.actor.clone(),
            occurred_at: now,
        };
        let tx = Transaction::new()
            .write(StateWrite::Action {
                action: action.clone(),
                expected: guard,
            })
            .audit(audit)
            .journal(journal);
        self.deps.uow.commit(tx).await?;
        if changed {
            crate::telemetry::record_action(action.status());
        }
        if publish == Publish::ActionChanged {
            self.publish(
                action,
                EventKind::ActionChanged {
                    action: action.id(),
                    status: action.status().clone(),
                },
            )
            .await;
        }
        Ok(())
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
        let shared = Arc::new(Shared {
            deps,
            settings,
            dispatches: Mutex::new(BTreeMap::new()),
            running: Mutex::new(BTreeSet::new()),
            writer: writer.clone(),
        });
        let writer_shared = shared.clone();
        let executor_shared = shared.clone();
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
            let _ = tokio::join!(
                writer::run(writer_shared, incoming),
                executor::serve(executor_shared, events),
            );
        });
        (ActionHandle { shared, writer }, task)
    }
}

impl ActionHandle {
    /// Approves action `id` on behalf of the human in `approval`. A chat approval needs
    /// `chat_token`, which works once. This is the only way to reach `Approved`; the
    /// decision is queued for the writer task, so it cannot race another one.
    pub async fn approve(
        &self,
        id: ActionId,
        approval: HumanApproval,
        chat_token: Option<&str>,
    ) -> Result<ActionRequest, ApprovalError> {
        let (reply, answer) = oneshot::channel();
        let command = writer::WriteCommand::Approve {
            id,
            approval,
            chat_token: chat_token.map(str::to_owned),
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

    /// Expires every proposed action whose window has closed and returns how
    /// many. The scheduler calls this periodically.
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
