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

mod autonomy;
mod briefing;
mod caller;
mod conversations;
mod helper_trust;
mod hosts;
mod incidents;
mod knowledge;
mod operations;
mod packages;
mod policy;
mod policy_publish;
mod policy_simulate;
mod policy_types;
mod preferences;

use std::sync::Arc;

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    AdvisoryRepo, Clock, HostRepo, IncidentRepo, MetricSampleRepo, OperationRepo, Redactor,
    SettingsRepo,
};
use domovoi_engine::ManagerHandle;
use domovoi_engine::operations::OperationHandle;
use domovoi_engine::triage::TriageHandle;

pub use autonomy::{AutonomyDeps, AutonomyView, SuspendedRule};
pub use briefing::{Briefing, BriefingError, BriefingHost, BriefingService};
pub use caller::{Caller, Surface};
pub use conversations::ConversationCommands;
pub use helper_trust::{HelperError, HelperTrust, HelperTrustDeps, HelperView};
pub use hosts::{HostService, HostStatus, OPEN_COUNT_CAP};
pub use incidents::{FollowUpError, IncidentService, RetriageError, Retriaged, latest_triage};
pub use knowledge::{KNOWLEDGE_PASSWORD_REQUIRED, KnowledgeCommandError, KnowledgeCommands};
pub use operations::{
    ANSWER_MAX, BudgetExtension, CANCEL_REASON_MAX, DEFAULT_CANCEL_REASON, EXTEND_MAX_COUNT,
    EXTEND_MAX_ENTRIES, EXTEND_MAX_MEASURED, OperationCommandError, OperationCommands, StepRef,
    SummaryCommandError, risk_of_skill,
};
pub use packages::{
    JobInput, JobStatus, JobView, JobsQuery, MAX_JOB_HOSTS,
    PASSWORD_REQUIRED_TEXT as PACKAGE_PASSWORD_REQUIRED_TEXT, PackageCommandError, PackageCommands,
    PackageDeps, PackageDetail, PackagesStatus, PlanInput, RetryInput, job_status,
};
pub use policy::PolicyService;
pub use policy_publish::PublishedVersion;
pub use policy_simulate::{
    InputsMode, RulesSource, SelectionChoice, SimulationItem, SimulationRequest, SimulationResult,
};
pub use policy_types::{
    BAD_PASSWORD_TEXT, PASSWORD_REQUIRED_TEXT, PolicyCommandError, PolicyDeps, Preview, RuleIssue,
    SavedDraft, StepUp,
};
pub use preferences::{
    MAX_PAGE_KEY_CHARS, MAX_PAGE_OVERRIDES, PREFERENCES_VERSION, PreferenceError,
    PreferenceService, Preferences, PreferencesUpdate, PresentationInput, PresentationPrefs,
    Profile, preferences_key,
};

use crate::agent_server::AgentAdmin;
use crate::profile::ProfileDeps;

/// Every service, built once from the values the runtime already holds.
#[derive(Clone)]
pub struct Services {
    /// Acknowledge, mute, ask and triage again.
    pub incidents: IncidentService,
    /// Approve and deny steps; cancel, pause, resume and answer operations.
    pub operations: OperationCommands,
    /// A host's status.
    pub hosts: HostService,
    /// The digest and profile summaries.
    pub briefing: BriefingService,
    /// Policy drafts, versions, previews and the simulator.
    pub policy: PolicyService,
    /// Facts, relations and entities the hub knows.
    pub knowledge: KnowledgeCommands,
    /// Operator conversations: create, ask, stop, retry, attach, delete.
    pub conversations: ConversationCommands,
    /// The signed-in person's own display preferences.
    pub preferences: PreferenceService,
    /// Signed packages: import, install jobs and what hosts report.
    pub packages: PackageCommands,
    /// The helper's key pin and attestation: view, re-check, pin and unpin.
    pub helper_trust: HelperTrust,
}

/// What the services are built from.
pub struct ServicesDeps {
    /// The incident manager.
    pub manager: ManagerHandle,
    /// Follow-up questions and triage; `None` when triage is off.
    pub triage: Option<TriageHandle>,
    /// The operation service; `None` unless `[orchestration] enabled` is set.
    pub operations: Option<OperationHandle>,
    /// Stored operations, read only.
    pub operation_repo: Arc<dyn OperationRepo>,
    /// Incidents, read only.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Hosts, read only.
    pub hosts: Arc<dyn HostRepo>,
    /// Digest facts.
    pub facts: Arc<dyn MetricSampleRepo>,
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
    /// The policy use cases.
    pub policy: PolicyService,
    /// The knowledge use cases.
    pub knowledge: KnowledgeCommands,
    /// The Operator conversation use cases.
    pub conversations: ConversationCommands,
    /// The settings table, where display preferences are kept.
    pub settings: Arc<dyn SettingsRepo>,
    /// The package use cases.
    pub packages: PackageCommands,
    /// The helper trust use cases.
    pub helper_trust: HelperTrust,
}

impl Services {
    /// The services over `deps`.
    #[must_use]
    pub fn new(deps: ServicesDeps) -> Self {
        let preferences = PreferenceService::new(deps.settings.clone(), deps.clock.clone());
        Self {
            incidents: IncidentService {
                manager: deps.manager,
                triage: deps.triage,
                operations: deps.operations.clone(),
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
            policy: deps.policy,
            knowledge: deps.knowledge,
            conversations: deps.conversations,
            preferences,
            packages: deps.packages,
            helper_trust: deps.helper_trust,
        }
    }
}
