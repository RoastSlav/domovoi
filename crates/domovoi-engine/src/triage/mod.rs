//! Triage: turns `TriageRequested` events into [`TriageOutcome`]s (ARCHITECTURE.md §11).
//!
//! A bounded queue feeds a few workers. Each run checks the hourly and monthly caps, then
//! the repeat cache, builds a redacted context bundle and runs a bounded loop over the
//! closed [`ReadTool`](domovoi_core::ReadTool) set. The final answer must be one JSON
//! [`TriageReport`](domovoi_core::TriageReport). The primary provider falls back to the
//! secondary; a failed run still leaves the card that already went out (INV-11). Valid
//! action proposals become `Proposed` actions; nothing the model writes is ever run (INV-9).

mod agent;
mod cache;
mod context;
mod followup;
mod proposals;
mod queue;
mod report;
mod run;
mod tools;

use std::sync::Arc;

use domovoi_core::ports::{
    ActionRepo, AdvisoryRepo, AgentLink, AuditLog, Clock, IncidentRepo, LlmError, LlmProvider,
    NoteRepo, Redactor, SignalRepo, TriageCacheRepo, UsageRepo,
};
use domovoi_core::{Incident, IncidentId, TriageBudget, TriageOutcome, UserId};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::events::DomainEvent;
use crate::manager::ManagerHandle;

pub use agent::{BUDGET_EXHAUSTED, NO_CALLS_LEFT, read_tools};
pub use followup::{FOLLOW_UP_PROMPT, MAX_ANSWER_CHARS};
pub use proposals::{PROPOSER, mint_token, token_hash};
pub use report::parse_report;
pub(crate) use tools::cut;
pub use tools::{MAX_RESULT_BYTES, TOOL_DEADLINE};

/// The system prompt for triage runs.
pub const SYSTEM_PROMPT: &str = include_str!("../../prompts/triage_system.md");

/// Requests waiting beyond this many drop the oldest, which is attached as skipped.
pub const DEFAULT_QUEUE: usize = 64;

/// How long a proposed action waits for a human before it expires.
pub const PROPOSAL_TTL: jiff::SignedDuration = jiff::SignedDuration::from_hours(1);

/// Consecutive failed runs that mark triage as degraded.
pub const DEGRADED_AFTER: u32 = 3;

/// How triage runs are limited. The hub fills it from `[triage]` in `hub.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriageSettings {
    /// Runs at the same time.
    pub concurrency: u8,
    /// Requests that may wait.
    pub queue: usize,
    /// Runs started per rolling hour at most.
    pub per_hour_runs: u32,
    /// Spending cap per calendar month (UTC), in millionths of a US dollar.
    pub monthly_budget_micros: Option<u64>,
    /// Days a report is reused for the same incident and evidence.
    pub cache_days: u32,
    /// Limits for one run.
    pub budget: TriageBudget,
}

impl Default for TriageSettings {
    fn default() -> Self {
        Self {
            concurrency: 2,
            queue: DEFAULT_QUEUE,
            per_hour_runs: 6,
            monthly_budget_micros: None,
            cache_days: 7,
            budget: TriageBudget {
                max_tool_calls: 15,
                max_seconds: 90,
                max_output_tokens: 2000,
            },
        }
    }
}

/// What triage needs. Everything is a port, so the engine does no I/O itself.
#[derive(Clone)]
pub struct TriageDeps {
    /// The provider tried first.
    pub primary: Arc<dyn LlmProvider>,
    /// The fallback when the primary is unavailable.
    pub secondary: Option<Arc<dyn LlmProvider>>,
    /// Read tools and skill catalogues on agents.
    pub link: Arc<dyn AgentLink>,
    /// Incidents, read only; changes go through `manager`.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Recent signals of an incident.
    pub signals: Arc<dyn SignalRepo>,
    /// Host notes.
    pub notes: Arc<dyn NoteRepo>,
    /// Where proposed actions are stored.
    pub actions: Arc<dyn ActionRepo>,
    /// Where `action.proposed` is recorded.
    pub audit: Arc<dyn AuditLog>,
    /// Reports reused for repeat incidents.
    pub cache: Arc<dyn TriageCacheRepo>,
    /// Token use and cost per call.
    pub usage: Arc<dyn UsageRepo>,
    /// The `TriageDegraded` advisory.
    pub advisories: Arc<dyn AdvisoryRepo>,
    /// Runs on every tool output, context part and answer.
    pub redactor: Arc<dyn Redactor>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// The incident manager, which stores outcomes.
    pub manager: ManagerHandle,
    /// The manager's event bus: requests come from it and `TriageDegraded` goes to it.
    pub bus: broadcast::Sender<DomainEvent>,
    /// Stops the service.
    pub shutdown: CancellationToken,
}

/// Why a follow-up question got no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TriageError {
    /// No incident has that id.
    #[error("no such incident {0}")]
    UnknownIncident(IncidentId),
    /// Every provider failed.
    #[error("the model is not available: {0}")]
    Provider(LlmError),
    /// The store failed.
    #[error("store error: {0}")]
    Store(String),
}

/// The triage service's state shared by its workers and handles.
pub(crate) struct Shared {
    pub(crate) deps: TriageDeps,
    pub(crate) settings: TriageSettings,
    pub(crate) limits: std::sync::Mutex<run::Limits>,
    pub(crate) queue: queue::Queue,
}

/// Reaches the running triage service. Cheap to clone.
#[derive(Clone)]
pub struct TriageHandle {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for TriageHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriageHandle")
            .field("settings", &self.shared.settings)
            .finish_non_exhaustive()
    }
}

/// The triage service.
#[derive(Debug)]
pub struct TriageService;

impl TriageService {
    /// Starts the service: it subscribes to `deps.bus` and runs `settings.concurrency`
    /// workers until `deps.shutdown` fires.
    #[must_use]
    pub fn spawn(deps: TriageDeps, settings: TriageSettings) -> (TriageHandle, JoinHandle<()>) {
        let events = deps.bus.subscribe();
        let shared = Arc::new(Shared {
            queue: queue::Queue::new(settings.queue.max(1)),
            limits: std::sync::Mutex::new(run::Limits::default()),
            deps,
            settings,
        });
        let task = tokio::spawn(queue::serve(shared.clone(), events));
        (TriageHandle { shared }, task)
    }
}

impl TriageHandle {
    /// Triages `incident` now, outside the queue but with the same caps, and attaches the
    /// outcome. The service does this for every `TriageRequested`.
    pub async fn triage(&self, incident: &Incident) -> TriageOutcome {
        run::triage(&self.shared, incident).await
    }

    /// Answers `question` about `incident` in plain text, with the same read tools and
    /// budget. Never proposes actions. The question and the answer go into the timeline.
    pub async fn follow_up(
        &self,
        incident: IncidentId,
        question: &str,
        asked_by: UserId,
    ) -> Result<String, TriageError> {
        followup::answer(&self.shared, incident, question, asked_by).await
    }

    /// Requests waiting for a worker.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.shared.queue.len()
    }
}
