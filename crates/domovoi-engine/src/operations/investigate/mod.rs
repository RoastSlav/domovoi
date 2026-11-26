//! Investigate operations: the triage of an incident, run as a durable operation.
//!
//! One cycle is a pipeline of four effects, each a thin wrapper over the triage code:
//! `BuildContext` (repeat cache, admission, context bundle), `CallModel` (provider chain,
//! tool loop, report), `DraftSummary` (proposal checks and the assessment, plan, execution
//! and notification steps) and `Notify` (the card and the timeline entry). The whole tool
//! loop of a run stays inside one `CallModel` effect and one reasoning turn.
//!
//! Every handler reads steps and the incident from the store and commits through one writer
//! command that also completes its own effect, so a restart at any point finishes the
//! investigation once.

mod build_context;
mod call_model;
mod close;
mod draft_plan;
mod draft_summary;
mod notify;
mod plumbing;
mod resume;
mod start;
mod turn_output;
mod waiters;

use std::sync::Arc;

use jiff::SignedDuration;
use tokio::sync::Mutex;

use super::OperationHandle;
use super::writer::OpShared;
use crate::effects::EffectRunner;
use crate::triage::{Shared, TriageHandle};

/// Attempts allowed to a model call.
const CALL_ATTEMPTS: u32 = 3;
/// Attempts allowed to a draft.
const DRAFT_ATTEMPTS: u32 = 3;
/// Attempts allowed to a notification.
const NOTIFY_ATTEMPTS: u32 = 5;
/// Attempts allowed to a retry's context build.
const BUILD_ATTEMPTS: u32 = 3;

pub use start::InvestigateError;
pub use turn_output::APPROVAL_POINTER;
pub(crate) use turn_output::{TurnKind, TurnOutput};
use waiters::Waiters;

/// What investigations are built from.
#[derive(Clone)]
pub struct InvestigateDeps {
    /// The triage service whose gates, context, provider chain and card an investigation
    /// reuses.
    pub triage: TriageHandle,
    /// How investigations are paced and what the model may do in them.
    pub settings: InvestigateSettings,
}

/// How investigations are paced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvestigateSettings {
    /// How often the closer scans open investigations.
    pub close_interval: SignedDuration,
    /// How long `investigate_and_wait` waits before giving up.
    pub wait_limit: SignedDuration,
    /// Incidents the start-up catch-up examines.
    pub catch_up_limit: u16,
    /// Whether the model is offered `ask_user`. Set only where a person can answer a
    /// question: the answer route must exist, or an asked question would wait for nothing.
    pub allow_questions: bool,
}

impl Default for InvestigateSettings {
    fn default() -> Self {
        Self {
            close_interval: SignedDuration::from_secs(30),
            wait_limit: SignedDuration::from_mins(20),
            catch_up_limit: 200,
            allow_questions: false,
        }
    }
}

/// What the investigation handlers and tasks share.
pub(crate) struct Runtime {
    /// The triage service's state.
    pub(crate) triage: Arc<Shared>,
    /// Pacing.
    pub(crate) settings: InvestigateSettings,
    /// Callers of `investigate_and_wait`.
    pub(crate) waiters: Waiters,
    /// Serialises "is one open?" and "create one", so two starts never both create.
    pub(crate) start_lock: Mutex<()>,
}

impl Runtime {
    /// The runtime of `deps`.
    pub(crate) fn new(deps: &InvestigateDeps) -> Self {
        Self {
            triage: deps.triage.shared(),
            settings: deps.settings,
            waiters: Waiters::default(),
            start_lock: Mutex::new(()),
        }
    }
}

/// Registers the four handlers on `runner`.
pub(super) fn register(runner: &mut EffectRunner, shared: &Arc<OpShared>) {
    use domovoi_core::ports::EffectKind;
    runner.register(
        EffectKind::BuildContext,
        Arc::new(build_context::BuildContext::new(shared.clone())),
    );
    runner.register(
        EffectKind::CallModel,
        Arc::new(call_model::CallModel::new(shared.clone())),
    );
    runner.register(
        EffectKind::DraftSummary,
        Arc::new(draft_summary::DraftSummary::new(shared.clone())),
    );
    runner.register(
        EffectKind::Notify,
        Arc::new(notify::Notify::new(shared.clone())),
    );
}

/// Runs the investigator and the closer until shutdown.
pub(super) async fn run(handle: OperationHandle) {
    let _ = tokio::join!(
        start::run_investigator(handle.clone()),
        close::run_closer(handle),
    );
}
