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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{
    ActionRepo, AgentLink, AuditEvent, AuditLog, Clock, IncidentRepo, Redactor, StoreError,
};
use domovoi_core::{ActionError, ActionId, ActionRequest, HostId, HumanApproval};
use jiff::{SignedDuration, Timestamp};
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
    /// The append-only audit log.
    pub audit: Arc<dyn AuditLog>,
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

pub(crate) struct Shared {
    pub(crate) deps: ActionDeps,
    pub(crate) settings: ActionSettings,
    pub(crate) dispatches: Mutex<Dispatches>,
    /// Actions being run right now, so one is never run twice at once.
    pub(crate) running: Mutex<BTreeSet<ActionId>>,
    /// The queue into the writer task. The executor hands every transition to that
    /// one writer (ADR 0019) rather than writing action state itself.
    pub(crate) writer: writer::WriterHandle,
}

impl Shared {
    /// Stores `action` through the guarded write, so a decision that read the
    /// action before it changed is refused rather than overwriting an outcome,
    /// appends `entry` to the audit log and publishes `ActionChanged`. The store's
    /// own error lets the caller tell a lost race from a real failure.
    pub(crate) async fn record_from(
        &self,
        action: &ActionRequest,
        expected: &str,
        entry: AuditEvent,
    ) -> Result<(), StoreError> {
        self.deps.actions.update_from(action, expected).await?;
        self.announce(action, entry).await;
        Ok(())
    }

    /// Appends `entry` to the audit log, counts the status and publishes
    /// `ActionChanged`. Only called once the store accepted the write.
    pub(crate) async fn announce(&self, action: &ActionRequest, entry: AuditEvent) {
        if let Err(err) = self.deps.audit.append(&entry).await {
            tracing::error!(%err, kind = %entry.kind, "could not write the audit log");
        }
        crate::telemetry::record_action(action.status());
        self.publish(
            action,
            EventKind::ActionChanged {
                action: action.id(),
                status: action.status().clone(),
            },
        )
        .await;
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
