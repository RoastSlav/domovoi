//! The hub's use-case layer: what a person can ask the hub to do, independent of where they
//! asked from.
//!
//! A transport (the HTTP API, the chat interactions, a webhook) does three things only:
//! authenticate and parse (turn a cookie or a button press into a typed request and a
//! [`Caller`]), call one service method, and render the answer in its own format. Everything
//! between (is this allowed, what exactly is written, which audit row) belongs to a service.
//!
//! Commands go through a service; plain repository reads may stay in the transport that
//! renders them, but a read shared by two transports lives in a service. A service never
//! imports a transport or the runtime, never spawns a task, never builds an approval or a
//! [`Caller`], and returns domain or engine types, never a DTO or an HTTP error.
//! `cargo xtask arch-check` enforces the import rules.

mod approvals;
mod briefing;
mod caller;
mod hosts;
mod incidents;
mod operations;

use std::sync::Arc;

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    ActionRepo, AdvisoryRepo, Clock, FactRepo, HostRepo, IncidentRepo, OperationRepo, Redactor,
};
use domovoi_engine::operations::OperationHandle;
use domovoi_engine::triage::TriageHandle;
use domovoi_engine::{ActionHandle, ManagerHandle};

pub use approvals::{ApprovalService, ApproveError, ApproveInput};
pub use briefing::{Briefing, BriefingError, BriefingHost, BriefingService};
pub use caller::{Caller, Surface};
pub use hosts::{HostService, HostStatus, OPEN_COUNT_CAP};
pub use incidents::{FollowUpError, IncidentService, RetriageError, Retriaged, latest_triage};
pub use operations::{
    ANSWER_MAX, BudgetExtension, CANCEL_REASON_MAX, DEFAULT_CANCEL_REASON, EXTEND_MAX_COUNT,
    EXTEND_MAX_ENTRIES, EXTEND_MAX_MEASURED, OperationCommandError, OperationCommands, StepRef,
    risk_of_skill,
};

use crate::agent_server::AgentAdmin;
use crate::profile::ProfileDeps;

/// Every service, built once from the values the runtime already holds.
#[derive(Clone)]
pub struct Services {
    /// Acknowledge, mute, ask and triage again.
    pub incidents: IncidentService,
    /// Approve and deny proposed actions.
    pub approvals: ApprovalService,
    /// Approve and deny steps; cancel, pause, resume and answer operations.
    pub operations: OperationCommands,
    /// A host's status.
    pub hosts: HostService,
    /// The digest and profile summaries.
    pub briefing: BriefingService,
}

/// What the services are built from.
pub struct ServicesDeps {
    /// The incident manager.
    pub manager: ManagerHandle,
    /// Follow-up questions and triage; `None` when triage is off.
    pub triage: Option<TriageHandle>,
    /// Actions, read only.
    pub actions: Arc<dyn ActionRepo>,
    /// The action approval service.
    pub actions_service: ActionHandle,
    /// The operation service; `None` unless `[orchestration] enabled` is set.
    pub operations: Option<OperationHandle>,
    /// Stored operations, read only.
    pub operation_repo: Arc<dyn OperationRepo>,
    /// Incidents, read only.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Hosts, read only.
    pub hosts: Arc<dyn HostRepo>,
    /// Digest facts.
    pub facts: Arc<dyn FactRepo>,
    /// Advisories.
    pub advisories: Arc<dyn AdvisoryRepo>,
    /// Presence and catalogues of agents.
    pub agents: Arc<dyn AgentAdmin>,
    /// The profile task's handles; `None` when the hub runs without one.
    pub profiles: Option<ProfileDeps>,
    /// The loaded configuration.
    pub config: Arc<HubConfig>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Removes secrets from text before it is stored.
    pub redactor: Arc<dyn Redactor>,
}

impl Services {
    /// The services over `deps`.
    #[must_use]
    pub fn new(deps: ServicesDeps) -> Self {
        Self {
            incidents: IncidentService {
                manager: deps.manager,
                triage: deps.triage,
                operations: deps.operations.clone(),
            },
            approvals: ApprovalService {
                actions: deps.actions,
                handle: deps.actions_service,
                clock: deps.clock.clone(),
            },
            operations: OperationCommands {
                handle: deps.operations,
                repo: deps.operation_repo,
                agents: deps.agents.clone(),
                redactor: deps.redactor,
                clock: deps.clock.clone(),
            },
            hosts: HostService {
                incidents: deps.incidents.clone(),
                hosts: deps.hosts.clone(),
                agents: deps.agents,
            },
            briefing: BriefingService {
                config: deps.config,
                clock: deps.clock,
                hosts: deps.hosts,
                facts: deps.facts,
                incidents: deps.incidents,
                advisories: deps.advisories,
                profiles: deps.profiles,
            },
        }
    }
}
