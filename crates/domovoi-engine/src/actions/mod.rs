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

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{
    ActionRepo, AgentLink, AuditEvent, AuditLog, Clock, IncidentRepo, Redactor,
};
use domovoi_core::{ActionError, ActionId, ActionRequest, HostId, HumanApproval};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::broadcast;
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
}

/// Run dispatch times per host and skill, for the loop guard.
type Dispatches = BTreeMap<(HostId, String), VecDeque<Timestamp>>;

pub(crate) struct Shared {
    pub(crate) deps: ActionDeps,
    pub(crate) settings: ActionSettings,
    pub(crate) dispatches: Mutex<Dispatches>,
    /// Actions being run right now, so one is never run twice at once.
    pub(crate) running: Mutex<BTreeSet<ActionId>>,
}

impl Shared {
    /// Stores `action`, appends `entry` to the audit log and publishes `ActionChanged`.
    pub(crate) async fn record(
        &self,
        action: &ActionRequest,
        entry: AuditEvent,
    ) -> Result<(), String> {
        self.deps
            .actions
            .update(action)
            .await
            .map_err(|e| e.to_string())?;
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
    /// Starts the executor, which runs every approved action until `deps.shutdown`.
    #[must_use]
    pub fn spawn(deps: ActionDeps, settings: ActionSettings) -> (ActionHandle, JoinHandle<()>) {
        let events = deps.bus.subscribe();
        let shared = Arc::new(Shared {
            deps,
            settings,
            dispatches: Mutex::new(BTreeMap::new()),
            running: Mutex::new(BTreeSet::new()),
        });
        let task = tokio::spawn(executor::serve(shared.clone(), events));
        (ActionHandle { shared }, task)
    }
}

impl ActionHandle {
    /// Approves action `id` on behalf of the human in `approval`. A chat approval needs
    /// `chat_token`, which works once. This is the only way to reach `Approved`.
    pub async fn approve(
        &self,
        id: ActionId,
        approval: HumanApproval,
        chat_token: Option<&str>,
    ) -> Result<ActionRequest, ApprovalError> {
        approvals::approve(&self.shared, id, approval, chat_token).await
    }

    /// Denies action `id` on behalf of the human in `approval`.
    pub async fn deny(
        &self,
        id: ActionId,
        approval: HumanApproval,
    ) -> Result<ActionRequest, ApprovalError> {
        approvals::deny(&self.shared, id, approval).await
    }

    /// Expires every proposed action whose window has closed and returns how many. The
    /// scheduler calls this periodically.
    pub async fn expire_due(&self) -> Result<usize, ApprovalError> {
        approvals::expire_due(&self.shared).await
    }
}
