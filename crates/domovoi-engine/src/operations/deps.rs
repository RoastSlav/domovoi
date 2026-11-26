//! What the operation service is built from: ports only. Nothing here hands out the raw
//! agent link; reading, handing off, reconciling and re-checking are separate capabilities.

use std::sync::Arc;

use domovoi_core::ports::{
    AdvisoryRepo, CapabilityRegistry, CheckRerun, Clock, EffectQueue, EffectRead, ExperienceRead,
    HandoffRunner, HostObservation, HostRepo, IncidentRepo, JournalStatus, OperationRepo, Redactor,
    TargetResolver, UnitOfWork,
};
use tokio_util::sync::CancellationToken;

use crate::evidence::Evidence;

/// What the operation service needs. Everything is a port; the engine does no I/O.
#[derive(Clone)]
pub struct OperationDeps {
    /// Stored operations, read only.
    pub operations: Arc<dyn OperationRepo>,
    /// One atomic commit for a decision's state, journal, audit and effects.
    pub uow: Arc<dyn UnitOfWork>,
    /// Claims and recovers durable effects.
    pub effects: Arc<dyn EffectQueue>,
    /// Reads the effect outbox, for start-up recovery.
    pub effect_read: Arc<dyn EffectRead>,
    /// Where a dead effect is reported.
    pub advisories: Arc<dyn AdvisoryRepo>,
    /// Incidents, read only, for the freshness check before a dispatch.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Enrolled hosts and their agent keys, read for the dispatch checks.
    pub hosts: Arc<dyn HostRepo>,
    /// The one read-capable handle to the agents.
    pub evidence: Evidence,
    /// Whether the hub offers a contract on a host.
    pub registry: Arc<dyn CapabilityRegistry>,
    /// The canonical identity a plan binds.
    pub resolver: Arc<dyn TargetResolver>,
    /// Catalogue reads of the live session.
    pub observer: Arc<dyn HostObservation>,
    /// The journal epoch and the agent's answers about runs.
    pub journal: Arc<dyn JournalStatus>,
    /// Skill dispatch and its terminal answer.
    pub handoff: Arc<dyn HandoffRunner>,
    /// The re-check that verifies a finished execution.
    pub rerun: Arc<dyn CheckRerun>,
    /// Resolution summaries, read to plan a draft or an edit and to find what is missing.
    pub experience: Arc<dyn ExperienceRead>,
    /// Runs on text before it is stored.
    pub redactor: Arc<dyn Redactor>,
    /// The event bus: a step leaving "awaiting approval" is announced on it.
    pub bus: tokio::sync::broadcast::Sender<crate::events::DomainEvent>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Stops the service.
    pub shutdown: CancellationToken,
    /// Runs investigations when present; without it `TriageRequested` is ignored.
    pub investigate: Option<super::InvestigateDeps>,
    /// Unit restarts and unattended permits; without it a `unit.restart` step cannot be
    /// dispatched and no permit is ever considered.
    pub autonomy: Option<AutonomyDeps>,
    /// Containment blocks; without it a `firewall.block_source` step cannot be proposed or
    /// dispatched.
    pub contain: Option<Arc<crate::containment::ContainmentDriver>>,
}

/// What bounded unattended execution needs beyond a person's approval.
#[derive(Clone)]
pub struct AutonomyDeps {
    /// Reads unit identities and hands a restart to its host's agent.
    pub relay: Arc<dyn domovoi_core::ports::UnitRelay>,
    /// The pause switch, the circuit breaker's inputs and the unknown-outcome block.
    pub permits: Arc<dyn domovoi_core::ports::PermitRepo>,
    /// Turns an enforced policy `Allow` into a stored permit; the only way one is made.
    pub issuer: Arc<dyn domovoi_core::ports::PermitIssuer>,
}
