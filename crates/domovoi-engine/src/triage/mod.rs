//! Triage: follow-up questions, chat and the investigation steps (ARCHITECTURE.md §11).
//!
//! This module serves follow-up questions and chat, and holds the steps an investigation
//! operation runs for one incident: the hourly and monthly caps, the repeat cache, a redacted
//! context bundle and a bounded loop over the closed [`MetaTool`](domovoi_core::MetaTool)
//! set, whose reads take only [`ReadTool`](domovoi_core::ReadTool) names. The final answer is
//! one [`TriageReport`](domovoi_core::TriageReport), through `finish` or as JSON text. The
//! primary provider falls back to the providers after it in order; a failed run still leaves
//! the card that already went out (INV-11). Nothing the model writes is ever run (INV-9).

pub(crate) mod admission;
pub(crate) mod agent;
pub(crate) mod cache;
mod chat;
mod complete;
pub(crate) mod context;
mod followup;
mod history;
mod incident_context;
pub(crate) mod meta;
mod prompt;
pub(crate) mod proposals;
pub(crate) mod report;
pub(crate) mod run;
pub(crate) mod tools;

use std::sync::Arc;

use domovoi_core::ports::{
    ActionRepo, AdmissionRepo, AdvisoryRepo, CapabilityRegistry, Clock, HostRepo, IncidentRepo,
    LlmProvider, NoteRepo, Redactor, SignalRepo, TargetResolver, TriageCacheRepo, UsageRepo,
};
use domovoi_core::{HostId, IncidentId, TriageBudget, UserId};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::events::DomainEvent;
use crate::evidence::{Evidence, HostCatalog};
use crate::manager::ManagerHandle;

pub use agent::{
    BUDGET_EXHAUSTED, MAX_OUTPUT_ALLOWANCE, NO_CALLS_LEFT, ToolUse, default_total_output_tokens,
};
pub use chat::{CHAT_PROMPT, ChatRole, ChatTurn};
pub use followup::{FOLLOW_UP_PROMPT, MAX_ANSWER_CHARS};
pub use history::{CLEAR_KIND, FOLLOW_UP_KIND, Turn, count_clear, since_clear};
pub use meta::{META_SYSTEM_PROMPT, offered_tools};
pub use report::parse_report;
pub(crate) use tools::cut;
pub use tools::{MAX_RESULT_BYTES, TOOL_DEADLINE, UNKNOWN_NOTE};

/// The system prompt for triage runs with a provider that cannot call tools.
pub const SYSTEM_PROMPT: &str = include_str!("../../prompts/triage_system.md");

/// Consecutive failed runs that mark triage as degraded.
pub const DEGRADED_AFTER: u32 = 3;

/// How long a failed triage waits before it runs again.
pub const FAILED_RETRY: jiff::SignedDuration = jiff::SignedDuration::from_hours(1);

/// Consecutive failed runs after which one incident stops retrying. The limit is per
/// incident, so a success elsewhere does not refresh it.
pub const FAILED_RETRY_LIMIT: usize = 3;

/// How triage runs are limited. The hub fills it from `[triage]` in `hub.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriageSettings {
    /// Questions and chat answered at the same time.
    pub concurrency: u8,
    /// Runs started per rolling hour at most.
    pub per_hour_runs: u32,
    /// Runs the hour always keeps free for triage: questions and chat may not use these
    /// last runs of the hour.
    pub reserved_for_incidents: u32,
    /// Spending cap per calendar month (UTC), in millionths of a US dollar.
    pub monthly_budget_micros: Option<u64>,
    /// Days a report is reused for the same incident and evidence.
    pub cache_days: u32,
    /// Largest prompt sent in one model call, in bytes. Old tool results are shortened
    /// first to fit.
    pub max_prompt_bytes: usize,
    /// Limits for one run.
    pub budget: TriageBudget,
}

impl Default for TriageSettings {
    fn default() -> Self {
        Self {
            concurrency: 2,
            per_hour_runs: 6,
            reserved_for_incidents: 2,
            monthly_budget_micros: None,
            cache_days: 7,
            max_prompt_bytes: 131_072,
            budget: TriageBudget {
                max_tool_calls: 15,
                max_seconds: 90,
                max_output_tokens: 2000,
                // Every normal turn, one full growth ladder, and the repair's ladder; see
                // `default_total_output_tokens`.
                max_total_output_tokens: default_total_output_tokens(2000, 15),
            },
        }
    }
}

/// The sentence a refused question shows: `retry_at` names the time the hourly model
/// limit frees an admissible slot; `None` means another question is being answered now.
fn busy_text(retry_at: &Option<jiff::Timestamp>) -> String {
    match retry_at {
        Some(at) => format!(
            "the hourly model limit is reached; try again after {}",
            at.to_zoned(jiff::tz::TimeZone::UTC).strftime("%H:%M UTC"),
        ),
        None => "another question is being answered; try again in a moment".to_owned(),
    }
}

/// What triage needs. Everything is a port, so the engine does no I/O itself.
#[derive(Clone)]
pub struct TriageDeps {
    /// The provider tried first.
    pub primary: Arc<dyn LlmProvider>,
    /// Providers tried in order after the primary fails.
    pub fallbacks: Vec<Arc<dyn LlmProvider>>,
    /// The one path to an agent's tool output.
    pub evidence: Evidence,
    /// The agent sessions, skills and presence triage needs beside its reads.
    pub catalog: HostCatalog,
    /// The capability registry: the skills the owner enables on each host.
    pub capabilities: Arc<dyn CapabilityRegistry>,
    /// Proves the identity of a host a suggested step names, before the step is staged.
    pub resolver: Arc<dyn TargetResolver>,
    /// Incidents, read only; changes go through `manager`.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Actions of an incident, read only: the follow-up context lists the legacy ones.
    pub actions: Arc<dyn ActionRepo>,
    /// Recent signals of an incident.
    pub signals: Arc<dyn SignalRepo>,
    /// Host notes.
    pub notes: Arc<dyn NoteRepo>,
    /// Enrolled hosts and their agent keys, for the v2 plan target.
    pub hosts: Arc<dyn HostRepo>,
    /// Reports reused for repeat incidents.
    pub cache: Arc<dyn TriageCacheRepo>,
    /// Token use and cost per call.
    pub usage: Arc<dyn UsageRepo>,
    /// The persisted hourly run window, shared by triage, questions and chat.
    pub admission: Arc<dyn AdmissionRepo>,
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
    /// Every provider failed; the text lists each provider's reason joined by "; ".
    #[error("the model is not available: {0}")]
    Provider(String),
    /// The monthly model budget is spent, so no provider was asked.
    #[error("the monthly model budget is spent")]
    BudgetSpent,
    /// The store failed.
    #[error("store error: {0}")]
    Store(String),
    /// The hourly model limit is reached, or another question is being answered. `retry_at`
    /// names the time an admissible slot frees when it is known.
    #[error("{busy}", busy = busy_text(retry_at))]
    Busy {
        /// When an admissible slot frees, or `None` when the refusal is transient (another
        /// question is being answered) or the hour can never admit a question.
        retry_at: Option<jiff::Timestamp>,
    },
    /// The model limits could not be read, so no model call was made.
    #[error("the model limits could not be checked; try again shortly")]
    AdmissionUnavailable,
}

/// The triage service's state shared by its workers and handles.
pub(crate) struct Shared {
    pub(crate) deps: TriageDeps,
    pub(crate) settings: TriageSettings,
    pub(crate) limits: std::sync::Mutex<run::Limits>,
    /// One answer in flight per worker: questions and chat hold a permit while their
    /// model call runs.
    pub(crate) questions: tokio::sync::Semaphore,
}

impl Shared {
    /// The providers a triage run tries, in order: the primary, then its fallbacks.
    pub(crate) fn chain(&self) -> Vec<Arc<dyn LlmProvider>> {
        std::iter::chain(
            std::iter::once(self.deps.primary.clone()),
            self.deps.fallbacks.iter().map(Arc::clone),
        )
        .collect()
    }
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
    /// Starts the service. It only waits for `deps.shutdown`: questions and chat run on
    /// the caller's task, and investigations run as operations.
    #[must_use]
    pub fn spawn(deps: TriageDeps, settings: TriageSettings) -> (TriageHandle, JoinHandle<()>) {
        let shutdown = deps.shutdown.clone();
        let shared = Arc::new(Shared {
            limits: std::sync::Mutex::new(run::Limits::default()),
            questions: tokio::sync::Semaphore::new(usize::from(settings.concurrency.max(1))),
            deps,
            settings,
        });
        let task = tokio::spawn(async move { shutdown.cancelled().await });
        (TriageHandle { shared }, task)
    }
}

impl TriageHandle {
    /// The shared state, for the investigation handlers that run the same steps as a
    /// queued run.
    pub(crate) fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
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

    /// Answers `question` about the operator's hosts in general, outside any incident
    /// thread. With `host` the model may use that host's read tools and read skills; with
    /// `None` it answers only from `overview`, the summary of every host, and calls no
    /// tools. `history` is the chat the operator already had. Nothing it writes is ever
    /// proposed or run.
    pub async fn chat(
        &self,
        host: Option<HostId>,
        overview: &str,
        history: &[ChatTurn],
        question: &str,
    ) -> Result<String, TriageError> {
        chat::answer(&self.shared, host, overview, history, question).await
    }

    /// The target resolver triage checks plan steps with, so the operation service can bind
    /// plans through the same one.
    #[must_use]
    pub fn resolver(&self) -> Arc<dyn TargetResolver> {
        self.shared.deps.resolver.clone()
    }

    /// The follow-up questions and answers the incident still remembers: the ones after
    /// the last clear marker, oldest first. Reading them takes no model call. An unknown
    /// incident fails with `TriageError::UnknownIncident` before the timeline read, so
    /// the hub can tell a cleared conversation from an incident nobody stored.
    pub async fn history(&self, incident: IncidentId) -> Result<Vec<Turn>, TriageError> {
        // The existence check first: an unknown incident's timeline is just empty,
        // which would otherwise read back as a cleared conversation.
        self.shared
            .deps
            .incidents
            .get(incident)
            .await
            .map_err(|err| TriageError::Store(err.to_string()))?
            .ok_or(TriageError::UnknownIncident(incident))?;
        let timeline = match self.shared.deps.incidents.timeline(incident).await {
            Ok(found) => found,
            Err(err) => return Err(TriageError::Store(err.to_string())),
        };
        Ok(since_clear(&timeline))
    }
}
