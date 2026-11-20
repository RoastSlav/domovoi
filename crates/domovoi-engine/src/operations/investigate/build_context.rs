//! `BuildContext`: the repeat cache, the admission gate and the context bundle of one cycle.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord};
use domovoi_core::{
    EdgeKind, FinishOutcome, IncidentStatus, OperationSnapshot, OperationStatus, StepId,
    StepIntent, StepKind, StepOutcome,
};
use serde_json::{Value, json};

use super::plumbing::{
    claim, conclude, end_operation, handle_of, incident_of, latest_turn, runtime, short, step_of,
};
use super::resume::{self, Next, Resume};
use super::turn_output::{TurnKind, TurnOutput};
use super::{CALL_ATTEMPTS, DRAFT_ATTEMPTS, Runtime};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::decide::{NewEdge, NewStep};
use crate::operations::effects::{Address, address, after, defer, finish, load, spec, unreadable};
use crate::operations::writer::{Conclusion, Held, OpShared};
use crate::triage::admission::{ADMISSION_RETRY, free_slots, next_slot, slot_after};
use crate::triage::context;
use crate::triage::run::{Prepared, prepare, refusal_text};

/// The handler of `build_context` effects.
pub(super) struct BuildContext {
    shared: Arc<OpShared>,
}

impl BuildContext {
    /// A handler over the operation service.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

/// What the effect asks for beyond its address.
struct Ask {
    cycle: u32,
    fresh: bool,
    retry: bool,
    resume: Option<Resume>,
}

fn parse(effect: &EffectRecord) -> Option<Ask> {
    let payload = &effect.payload;
    Some(Ask {
        cycle: u32::try_from(payload.get("cycle")?.as_u64()?).ok()?,
        fresh: payload
            .get("fresh")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        retry: payload
            .get("retry")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        resume: Resume::of(effect),
    })
}

#[async_trait]
impl EffectHandler for BuildContext {
    async fn handle(&self, effect: &EffectRecord, token: ClaimToken) -> EffectOutcome {
        let shared = &self.shared;
        let held = claim(effect, token);
        let (Some(Address { operation, .. }), Some(ask), Some(runtime)) =
            (address(effect), parse(effect), runtime(shared))
        else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        if snapshot.operation.status.is_terminal() {
            return finish(shared, held).await;
        }
        // An operation that waits for a slot, or that a person paused, spends nothing yet.
        if matches!(
            snapshot.operation.status,
            OperationStatus::Queued | OperationStatus::Paused { .. }
        ) {
            let at = after(shared, shared.settings.retry_interval);
            return again(shared, held, effect, at).await;
        }
        let turn = match open_turn(shared, &snapshot, &ask).await {
            Ok(Some(turn)) => turn,
            Ok(None) => return finish(shared, held).await,
            Err(done) => return done,
        };
        run(shared, &runtime, held, effect, &snapshot, turn, &ask).await
    }
}

/// Creates the same effect again, due at `at`.
async fn again(
    shared: &Arc<OpShared>,
    held: Held,
    effect: &EffectRecord,
    at: jiff::Timestamp,
) -> EffectOutcome {
    let Some(Address { operation, .. }) = address(effect) else {
        return unreadable();
    };
    let next = spec(
        EffectKind::BuildContext,
        effect.payload.clone(),
        at,
        effect.max_attempts,
        effect.dedup_key.clone(),
    );
    defer(shared, held, operation, None, next, false).await
}

/// The cycle's running turn, proposing it when the cycle has none yet. `None` when the cycle's
/// turn is already final, so the effect has nothing left to do.
async fn open_turn(
    shared: &Arc<OpShared>,
    snapshot: &OperationSnapshot,
    ask: &Ask,
) -> Result<Option<StepId>, EffectOutcome> {
    let found = match ask.resume {
        Some(resumed) => match resume::next(snapshot, resumed) {
            Next::Running(turn) => return Ok(Some(turn)),
            Next::Nothing => return Ok(None),
            Next::Propose => None,
        },
        None => latest_turn(snapshot, Some(ask.cycle)),
    };
    match found {
        Some(turn) if turn.progress.outcome == StepOutcome::Running => Ok(Some(turn.step.id)),
        Some(_) => Ok(None),
        None => {
            let operation = snapshot.operation.id;
            let id = StepId::new();
            let edges = match ask.resume {
                Some(resumed) => vec![NewEdge {
                    from: id,
                    to: resumed.question,
                    kind: EdgeKind::CausedBy,
                }],
                None => snapshot
                    .steps
                    .iter()
                    .rev()
                    .find(|record| record.step.kind == StepKind::Verification)
                    .map(|verification| {
                        vec![NewEdge {
                            from: id,
                            to: verification.step.id,
                            kind: EdgeKind::CausedBy,
                        }]
                    })
                    .unwrap_or_default(),
            };
            let step = NewStep {
                id,
                kind: StepKind::ReasoningTurn,
                phase: "investigate".to_owned(),
                parent: None,
                intent: StepIntent::ReasoningTurn {
                    context_refs: Vec::new(),
                },
            };
            if let Err(err) = handle_of(shared)
                .propose(operation, vec![step], edges)
                .await
            {
                return Err(EffectOutcome::Retry {
                    error: err.to_string(),
                });
            }
            // A turn the budget refuses is born final, and the operation is paused.
            match load(shared, operation).await {
                Ok(fresh) => Ok(step_of(&fresh, id)
                    .filter(|record| record.progress.outcome == StepOutcome::Running)
                    .map(|_| id)),
                Err(done) => Err(done),
            }
        }
    }
}

/// Runs the gates and builds the bundle for the open `turn`.
async fn run(
    shared: &Arc<OpShared>,
    runtime: &Runtime,
    held: Held,
    effect: &EffectRecord,
    snapshot: &OperationSnapshot,
    turn: StepId,
    ask: &Ask,
) -> EffectOutcome {
    let operation = snapshot.operation.id;
    let triage = &runtime.triage;
    let incident = match incident_of(runtime, snapshot).await {
        Ok(Some(incident)) => incident,
        Ok(None) => {
            return end_operation(
                shared,
                held,
                operation,
                FinishOutcome::Failed,
                "the incident no longer exists",
            )
            .await;
        }
        Err(done) => return done,
    };
    if ask.retry {
        match retry_gate(shared, runtime, held, effect, &incident).await {
            Gate::Go => {}
            Gate::Done(done) => return done,
        }
    }
    let skip_cache = ask.fresh || ask.cycle > 0 || ask.retry || ask.resume.is_some();
    match prepare(triage, &incident, skip_cache).await {
        Prepared::Cached { report, from } => {
            let mut output = TurnOutput::of(TurnKind::Cached);
            output.from = Some(from);
            let summary = short(&report.headline);
            output.report = Some(report);
            let ended = Conclusion {
                step: turn,
                outcome: StepOutcome::Succeeded,
                summary: Some(summary),
                reason: None,
                output: Some(output.to_json()),
                usage: None,
            };
            conclude(
                shared,
                held,
                operation,
                Some(ended),
                vec![draft(shared, operation, turn)],
            )
            .await
        }
        Prepared::Refused(refusal) => {
            let (text, retry_at) = refusal_text(triage, &refusal);
            let mut output = TurnOutput::of(TurnKind::Skipped);
            output.reason = Some(text.clone());
            output.retry_at = retry_at;
            let ended = Conclusion {
                step: turn,
                outcome: StepOutcome::NotRun,
                summary: None,
                reason: Some(short(&text)),
                output: Some(output.to_json()),
                usage: None,
            };
            conclude(
                shared,
                held,
                operation,
                Some(ended),
                vec![draft(shared, operation, turn)],
            )
            .await
        }
        Prepared::Admitted => {
            let skills = triage.deps.capabilities.enabled_skills(incident.host());
            let mut bundle = context::build(triage, &incident, &skills).await;
            if let Some(resumed) = ask.resume {
                bundle.push_str(&resume::section(triage, snapshot, resumed));
            }
            let call = spec(
                EffectKind::CallModel,
                json!({
                    "v": 1,
                    "operation": operation.to_string(),
                    "step": turn.to_string(),
                    "cycle": ask.cycle,
                    "context": bundle,
                }),
                after(shared, jiff::SignedDuration::ZERO),
                CALL_ATTEMPTS,
                Some(format!("call:{turn}")),
            );
            defer(shared, held, operation, Some(turn), call, false).await
        }
    }
}

/// The `draft_summary` effect for `turn`, with no tool use to report.
fn draft(
    shared: &OpShared,
    operation: domovoi_core::OperationId,
    turn: StepId,
) -> crate::operations::decide::EffectSpec {
    spec(
        EffectKind::DraftSummary,
        json!({ "v": 1, "operation": operation.to_string(), "step": turn.to_string(), "tools": [] }),
        after(shared, jiff::SignedDuration::ZERO),
        DRAFT_ATTEMPTS,
        Some(format!("draft:{turn}")),
    )
}

/// Whether a retry may go on to the model now.
enum Gate {
    Go,
    Done(EffectOutcome),
}

/// What a retry checks before it takes a run: the incident still needs one, and the hour has
/// a free slot. An incident that is resolved, muted, pending, or already triaged by a newer
/// request ends the investigation quietly.
async fn retry_gate(
    shared: &Arc<OpShared>,
    runtime: &Runtime,
    held: Held,
    effect: &EffectRecord,
    incident: &domovoi_core::Incident,
) -> Gate {
    let Some(operation) = incident_operation(effect) else {
        return Gate::Done(unreadable());
    };
    if matches!(
        incident.status(),
        IncidentStatus::Pending | IncidentStatus::Resolved | IncidentStatus::Muted { .. }
    ) {
        return Gate::Done(quiet(shared, held, operation).await);
    }
    let entries = match runtime.triage.deps.incidents.timeline(incident.id()).await {
        Ok(entries) => entries,
        Err(err) => {
            return Gate::Done(EffectOutcome::Retry {
                error: err.to_string(),
            });
        }
    };
    let newest = entries
        .iter()
        .rev()
        .map(|entry| entry.kind.as_str())
        .find(|kind| {
            kind.starts_with("triage.") && *kind != "triage.followup" && *kind != "triage.tools"
        });
    if newest.is_some_and(|kind| kind != "triage.skipped" && kind != "triage.failed") {
        return Gate::Done(quiet(shared, held, operation).await);
    }
    let now = runtime.triage.deps.clock.now();
    let at = match free_slots(&runtime.triage, now).await {
        Ok(0) => match next_slot(&runtime.triage).await {
            Ok(at) => at.or_else(|| slot_after(now)),
            Err(_) => now.checked_add(ADMISSION_RETRY).ok(),
        },
        Ok(_) => return Gate::Go,
        Err(_) => now.checked_add(ADMISSION_RETRY).ok(),
    };
    Gate::Done(again(shared, held, effect, at.unwrap_or(now)).await)
}

/// The operation the effect is addressed to.
fn incident_operation(effect: &EffectRecord) -> Option<domovoi_core::OperationId> {
    address(effect).map(|a| a.operation)
}

/// Ends the investigation of an incident that needs none.
async fn quiet(
    shared: &Arc<OpShared>,
    held: Held,
    operation: domovoi_core::OperationId,
) -> EffectOutcome {
    end_operation(
        shared,
        held,
        operation,
        FinishOutcome::Succeeded,
        "no investigation needed",
    )
    .await
}
