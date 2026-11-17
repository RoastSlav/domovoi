//! What the action services are built from: their settings and the ports they reach.
//!
//! Every capability the engine acts through is a capability object from `domovoi_core::ports`
//! (ADR 0023). None of them hands out the raw agent link: availability, the host catalogue,
//! the journal, the skill handoff and the check re-run are separate traits, and the hub
//! composes them from its own transport.

use std::sync::Arc;

use domovoi_core::ports::{
    ActionRepo, AdvisoryRepo, AuditLog, AvailabilityProbe, CheckRerun, Clock, EffectQueue,
    HandoffRunner, HostObservation, HostRepo, IncidentRepo, JournalStatus, Redactor, UnitOfWork,
};
use jiff::SignedDuration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::events::DomainEvent;
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
///
/// The agent-facing fields are capabilities, not the transport: the executor asks whether a
/// host is reachable, reads the catalogue, opens a handoff, waits for its result and re-runs
/// a check, and each of those is its own narrow trait.
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
    /// Whether a host's agent is reachable, polled by the host wait.
    pub probe: Arc<dyn AvailabilityProbe>,
    /// Catalogue reads of the live session before dispatch.
    pub observer: Arc<dyn HostObservation>,
    /// The journal epoch and the agent's answers about actions.
    pub journal: Arc<dyn JournalStatus>,
    /// Skill dispatch and its terminal answer.
    pub handoff: Arc<dyn HandoffRunner>,
    /// The re-check that verifies a finished action.
    pub rerun: Arc<dyn CheckRerun>,
    /// Enrolled hosts and their agent keys, read for the v2 target.
    pub hosts: Arc<dyn HostRepo>,
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
