//! Starting investigations: from a caller, from a `TriageRequested` event and at start-up.

use std::sync::Arc;

use domovoi_core::ports::{AggregateRef, EffectKind, EffectStatus, IncidentFilter, Page, Redactor};
use domovoi_core::triage::current_episode;
use domovoi_core::{
    Actor, BudgetLimits, Incident, IncidentStatus, OperationId, OperationKind, PinnedSubject,
    SYSTEM_OPERATIONS, StepId, StepIntent, StepKind, TriageOutcome,
};
use serde_json::json;
use tokio::sync::broadcast;

use super::Runtime;
use super::plumbing::cut as cut_chars;
use crate::events::EventKind;
use crate::operations::decide::NewStep;
use crate::operations::error::OperationError;
use crate::operations::handle::{OperationHandle, OperationSeed};

/// The seven parts of the context bundle, in order, as a turn names what it was given.
const CONTEXT_REFS: [&str; 7] = [
    "incident",
    "host_profile",
    "signals",
    "changes",
    "similar",
    "notes",
    "skills",
];

/// The extra part name on the first turn of an investigation a person asked for. Such a run is
/// not retried on its own after it fails or is skipped: it ends, and the person sees why.
pub(crate) const MANUAL_REF: &str = "requested";

/// Why `investigate_and_wait` did not return an outcome.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InvestigateError {
    /// An investigation is already open for the incident.
    #[error("an investigation is already running for this incident")]
    AlreadyRunning(OperationId),
    /// The investigation is still running; its outcome appears on the incident when it ends.
    #[error("the investigation is still running; its result appears on the incident")]
    TimedOut(OperationId),
    /// The operation service refused or failed.
    #[error("{0}")]
    Operation(#[from] OperationError),
}

/// The operation an investigation of `incident` starts from. The goal is redacted; the token
/// and cost limits are off, because the run's own budget, the hourly cap and the monthly
/// budget already bound spending, and a whole tool loop is one turn that would otherwise trip
/// an operation limit before its proposals are stored.
pub(crate) fn seed_for(
    incident: &Incident,
    fresh: bool,
    manual: bool,
    redactor: &dyn Redactor,
    defaults: &BudgetLimits,
) -> OperationSeed {
    let goal = cut_chars(
        &redactor.redact(&format!("Investigate: {}", incident.title())),
        500,
    );
    let goal = if goal.trim().is_empty() {
        "Investigate an incident".to_owned()
    } else {
        goal
    };
    OperationSeed {
        kind: OperationKind::Investigate,
        goal,
        pinned: vec![PinnedSubject {
            host: incident.host(),
            check: incident.check().clone(),
            target: incident.target().cloned(),
        }],
        incidents: vec![incident.id()],
        parent: None,
        budgets: Some(BudgetLimits {
            tokens: None,
            cost_micros: None,
            ..defaults.clone()
        }),
        first_steps: vec![NewStep {
            id: StepId::new(),
            kind: StepKind::ReasoningTurn,
            phase: "investigate".to_owned(),
            parent: None,
            intent: StepIntent::ReasoningTurn {
                context_refs: CONTEXT_REFS
                    .iter()
                    .chain(manual.then_some(&MANUAL_REF))
                    .map(|part| (*part).to_owned())
                    .collect(),
            },
        }],
        effects: vec![(
            EffectKind::BuildContext,
            json!({ "v": 1, "cycle": 0, "fresh": fresh, "retry": false, "manual": manual }),
        )],
        actor: Actor::System(SYSTEM_OPERATIONS.to_owned()),
    }
}

impl OperationHandle {
    /// Starts an Investigate operation for `incident`, or returns the id of the one already
    /// open for it.
    pub async fn investigate(
        &self,
        incident: &Incident,
        fresh: bool,
    ) -> Result<OperationId, OperationError> {
        self.start_investigation(incident, fresh, false, false)
            .await
            .map(|(id, _)| id)
    }

    /// `investigate`, then waits for the first outcome the investigation attaches to the
    /// incident. An already-open investigation returns `InvestigateError::AlreadyRunning`;
    /// waiting longer than the settings allow returns `InvestigateError::TimedOut`.
    pub async fn investigate_and_wait(
        &self,
        incident: &Incident,
        fresh: bool,
    ) -> Result<TriageOutcome, InvestigateError> {
        self.wait_for_outcome(incident, fresh, false).await
    }

    /// `investigate_and_wait` for a run a person asked for: it does not retry by itself. When
    /// the run is skipped or fails, the operation ends `failed` with the cause.
    pub async fn investigate_on_request_and_wait(
        &self,
        incident: &Incident,
        fresh: bool,
    ) -> Result<TriageOutcome, InvestigateError> {
        self.wait_for_outcome(incident, fresh, true).await
    }

    async fn wait_for_outcome(
        &self,
        incident: &Incident,
        fresh: bool,
        manual: bool,
    ) -> Result<TriageOutcome, InvestigateError> {
        let runtime = self.runtime()?;
        // Registered before the operation exists, so an outcome attached at once is not
        // missed.
        let (ticket, outcome) = runtime.waiters.register(incident.id());
        let (id, created) = match self
            .start_investigation(incident, fresh, true, manual)
            .await
        {
            Ok(started) => started,
            Err(err) => {
                runtime.waiters.forget(incident.id(), ticket);
                return Err(err.into());
            }
        };
        if !created {
            runtime.waiters.forget(incident.id(), ticket);
            return Err(InvestigateError::AlreadyRunning(id));
        }
        let clock = self.shared.deps.clock.clone();
        let now = clock.now();
        let limit = now.checked_add(runtime.settings.wait_limit).unwrap_or(now);
        tokio::select! {
            got = outcome => got.map_err(|_| InvestigateError::Operation(OperationError::Unavailable)),
            () = clock.sleep_until(limit) => {
                runtime.waiters.forget(incident.id(), ticket);
                Err(InvestigateError::TimedOut(id))
            }
        }
    }

    /// The investigation runtime, or `Unavailable` when this service has none.
    fn runtime(&self) -> Result<Arc<Runtime>, OperationError> {
        self.shared
            .investigate
            .clone()
            .ok_or(OperationError::Unavailable)
    }

    /// Starts an investigation unless one is open; the flag says whether one was created.
    ///
    /// An open investigation that is only waiting out a retry (its turn was skipped or failed
    /// and the next one is parked) is cancelled and replaced when `supersede_parked` is set,
    /// which a person asking for a run sets. Without that, the retry's fixed due time would
    /// answer every request with "already running" for up to an hour. The replacement runs
    /// through the same admission limits, so a spent hourly limit skips it again.
    async fn start_investigation(
        &self,
        incident: &Incident,
        fresh: bool,
        supersede_parked: bool,
        manual: bool,
    ) -> Result<(OperationId, bool), OperationError> {
        let runtime = self.runtime()?;
        let _one_at_a_time = runtime.start_lock.lock().await;
        let existing = self
            .shared
            .deps
            .operations
            .for_incident(incident.id())
            .await
            .map_err(|err| OperationError::Store(err.to_string()))?;
        if let Some(open) = existing
            .iter()
            .find(|op| op.kind == OperationKind::Investigate && !op.status.is_terminal())
        {
            let parked = supersede_parked && self.is_parked_retry(open.id).await?;
            if !parked {
                return Ok((open.id, false));
            }
            self.cancel(
                open.id,
                Actor::System(SYSTEM_OPERATIONS.to_owned()),
                "replaced by a new investigation a person asked for".to_owned(),
            )
            .await?;
        }
        let seed = seed_for(
            incident,
            fresh,
            manual,
            self.shared.deps.redactor.as_ref(),
            &self.shared.settings.budgets,
        );
        self.create(seed).await.map(|id| (id, true))
    }
}

impl OperationHandle {
    /// Whether `operation` is only waiting out a retry: a skipped or failed turn scheduled a
    /// `build_context` retry that is not due yet, and nothing has started the model call.
    async fn is_parked_retry(&self, operation: OperationId) -> Result<bool, OperationError> {
        let now = self.shared.deps.clock.now();
        let pending = self
            .shared
            .deps
            .effect_read
            .effects_with_status(EffectStatus::Pending, 500)
            .await
            .map_err(|err| OperationError::Store(err.to_string()))?;
        Ok(pending.iter().any(|effect| {
            effect.aggregate == AggregateRef::operation(operation)
                && effect.kind == EffectKind::BuildContext.as_str()
                && effect.payload["retry"] == true
                && effect.not_before > now
        }))
    }
}

/// Turns `TriageRequested` events into investigations until shutdown. At start, and after the
/// bus lagged, it first catches up on incidents that have no triage entry yet.
pub(super) async fn run_investigator(handle: OperationHandle) {
    let shared = handle.shared.clone();
    let Some(runtime) = shared.investigate.clone() else {
        return;
    };
    let mut events = runtime.triage.deps.bus.subscribe();
    catch_up(&handle, &runtime, true).await;
    let shutdown = shared.deps.shutdown.clone();
    loop {
        let event = tokio::select! {
            () = shutdown.cancelled() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) if event.kind == EventKind::TriageRequested && event.notify => {
                if let Err(err) = handle.investigate(&event.incident, false).await {
                    tracing::warn!(%err, incident = %event.incident.id(), "could not start an investigation");
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "investigations missed events; catching up");
                catch_up(&handle, &runtime, false).await;
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

/// Starts an investigation for each active incident that has no triage entry yet and none
/// open. At start only `Open` incidents are taken (an acknowledged or muted one was handled
/// by a person or a rule); after a lag the others are too. `Pending` ones never.
async fn catch_up(handle: &OperationHandle, runtime: &Runtime, only_open: bool) {
    let incidents = &runtime.triage.deps.incidents;
    let filter = IncidentFilter {
        active: Some(true),
        ..IncidentFilter::default()
    };
    // Known conditions are history: an incident one covers is not investigated, whatever
    // path it took here.
    let rules = match runtime.triage.deps.acks.list_rules().await {
        Ok(rules) => rules,
        Err(err) => {
            tracing::warn!(%err, "could not read known conditions; not starting investigations");
            return;
        }
    };
    let now = runtime.triage.deps.clock.now();
    let mut page = Page::first(Page::MAX_LIMIT);
    let mut started: u32 = 0;
    loop {
        let got = match incidents.list(&filter, &page).await {
            Ok(got) => got,
            Err(err) => {
                tracing::warn!(%err, "could not list incidents to investigate");
                return;
            }
        };
        for incident in got.items {
            let status = incident.status();
            if status == IncidentStatus::Pending || (only_open && status != IncidentStatus::Open) {
                continue;
            }
            let covered = rules.iter().any(|rule| {
                rule.covers_incident(
                    incident.host(),
                    incident.check(),
                    incident.target().map(|t| t.as_str()),
                    now,
                )
            });
            if covered {
                tracing::debug!(incident = %incident.id(), "a known condition covers it; no investigation");
                continue;
            }
            let done = match incidents.timeline(incident.id()).await {
                Ok(entries) => current_episode(incident.check(), &entries)
                    .iter()
                    .any(|entry| {
                        matches!(
                            entry.kind.as_str(),
                            "triage.report" | "triage.cached" | "triage.skipped" | "triage.failed"
                        )
                    }),
                Err(err) => {
                    tracing::warn!(%err, incident = %incident.id(), "could not inspect triage history");
                    continue;
                }
            };
            if done {
                continue;
            }
            // The limit bounds the investigations one pass starts, not the incidents it
            // reads, so a long list of handled incidents cannot hide an open one.
            if started >= u32::from(runtime.settings.catch_up_limit) {
                return;
            }
            started += 1;
            if let Err(err) = handle.investigate(&incident, false).await {
                tracing::warn!(%err, incident = %incident.id(), "could not start an investigation");
            }
        }
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return,
        }
    }
}
