//! `DraftSummary`: the assessment, plan, execution proposals and notification of a finished
//! turn, and the retry a skipped or failed turn earns.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord};
use domovoi_core::{
    EdgeKind, FinishOutcome, OperationId, OperationSnapshot, StepId, StepIntent, StepKind,
    StepOutcome, StepRecord,
};
use serde_json::{Value, json};

use super::draft_plan::store_plan;
use super::plumbing::cut as cut_chars;
use super::plumbing::{
    claim, conclude, end_operation, handle_of, incident_of, latest_turn, runtime, step_of,
};
use super::start::MANUAL_REF;
use super::turn_output::{TurnKind, TurnOutput, caused_by, output_of};
use super::{BUILD_ATTEMPTS, NOTIFY_ATTEMPTS};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::decide::{EffectSpec, NewEdge, NewStep};
use crate::operations::effects::{Address, address, after, finish, load, spec, unreadable};
use crate::operations::writer::{Held, OpShared};
use crate::triage::run::limits;
use crate::triage::{FAILED_RETRY, FAILED_RETRY_LIMIT};

/// The handler of `draft_summary` effects.
pub(super) struct DraftSummary {
    shared: Arc<OpShared>,
}

impl DraftSummary {
    /// A handler over the operation service.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

fn retry(error: impl ToString) -> EffectOutcome {
    EffectOutcome::Retry {
        error: error.to_string(),
    }
}

#[async_trait]
impl EffectHandler for DraftSummary {
    async fn handle(&self, effect: &EffectRecord, token: ClaimToken) -> EffectOutcome {
        let shared = &self.shared;
        let held = claim(effect, token);
        let (Some(Address { operation, step }), Some(runtime)) = (address(effect), runtime(shared))
        else {
            return unreadable();
        };
        let Some(turn_id) = step else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        let Some(turn) = step_of(&snapshot, turn_id)
            .filter(|record| record.progress.outcome.is_final())
            .cloned()
        else {
            return finish(shared, held).await;
        };
        if snapshot.operation.status.is_terminal() {
            return finish(shared, held).await;
        }
        let output = output_of(&turn).unwrap_or_else(|| {
            let mut output = TurnOutput::of(TurnKind::Failed);
            output.reason.clone_from(&turn.progress.reason);
            output
        });
        match incident_of(&runtime, &snapshot).await {
            Ok(Some(_)) => {}
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
        }
        let triage = &runtime.triage;
        let handle = handle_of(shared);
        if output.kind == TurnKind::Asked {
            return ask_owner(shared, held, operation, &snapshot, &turn, &output).await;
        }
        // The lines of the suggestions the model never corrected come first.
        let mut dropped: Vec<String> = output.dropped.clone();

        if let Some(report) = &output.report {
            if caused_by(&snapshot, turn_id, StepKind::Assessment).is_empty() {
                let assessment = StepId::new();
                let text = cut_chars(&format!("{}\n{}", report.summary, report.cause), 2000);
                let mut steps = vec![NewStep {
                    id: assessment,
                    kind: StepKind::Assessment,
                    phase: "assess".to_owned(),
                    parent: Some(turn_id),
                    intent: StepIntent::Assessment { text },
                }];
                let mut edges = vec![NewEdge {
                    from: assessment,
                    to: turn_id,
                    kind: EdgeKind::CausedBy,
                }];
                let mut plan = report.next_step.clone();
                for manual in &report.manual_steps {
                    if !plan.is_empty() {
                        plan.push('\n');
                    }
                    plan.push_str(manual);
                }
                if !plan.trim().is_empty() {
                    let id = StepId::new();
                    steps.push(NewStep {
                        id,
                        kind: StepKind::Plan,
                        phase: "plan".to_owned(),
                        parent: Some(assessment),
                        intent: StepIntent::Plan {
                            text: cut_chars(&plan, 2000),
                        },
                    });
                    edges.push(NewEdge {
                        from: id,
                        to: assessment,
                        kind: EdgeKind::CausedBy,
                    });
                }
                if let Err(err) = handle.propose(operation, steps, edges).await {
                    return retry(err);
                }
            }
            match store_plan(shared, triage, operation, turn_id, &output, report).await {
                Ok(lines) => dropped.extend(lines),
                Err(done) => return done,
            }
        }

        if caused_by(&snapshot, turn_id, StepKind::Notification).is_empty() {
            let subject = cut_chars(&output.headline(), 280);
            let subject = if subject.trim().is_empty() {
                "triage outcome".to_owned()
            } else {
                subject
            };
            let id = StepId::new();
            let step = NewStep {
                id,
                kind: StepKind::Notification,
                phase: "notify".to_owned(),
                parent: None,
                intent: StepIntent::Notification {
                    channel: "incident".to_owned(),
                    subject,
                },
            };
            let edge = NewEdge {
                from: id,
                to: turn_id,
                kind: EdgeKind::CausedBy,
            };
            if let Err(err) = handle.propose(operation, vec![step], vec![edge]).await {
                return retry(err);
            }
        }

        let mut successors = Vec::new();
        let mut retry_note = None;
        if let Some(due) = retry_due(&snapshot, &turn, &output, shared, triage) {
            retry_note = retry_text(&snapshot, &output, due);
            let cycle = turn.step.cycle;
            if latest_turn(&snapshot, None)
                .is_none_or(|latest| latest.step.ordinal <= turn.step.ordinal)
            {
                let id = StepId::new();
                let next = NewStep {
                    id,
                    kind: StepKind::ReasoningTurn,
                    phase: "investigate".to_owned(),
                    parent: None,
                    intent: StepIntent::ReasoningTurn {
                        context_refs: Vec::new(),
                    },
                };
                let edge = NewEdge {
                    from: id,
                    to: turn_id,
                    kind: EdgeKind::CausedBy,
                };
                if let Err(err) = handle.propose(operation, vec![next], vec![edge]).await {
                    return retry(err);
                }
            }
            successors.push(spec(
                EffectKind::BuildContext,
                json!({
                    "v": 1,
                    "operation": operation.to_string(),
                    "cycle": cycle,
                    "fresh": false,
                    "retry": true,
                }),
                due,
                BUILD_ATTEMPTS,
                Some(format!("build:{operation}:{cycle}:retry:{turn_id}")),
            ));
        }
        successors.push(notify(
            shared,
            operation,
            turn_id,
            effect,
            &dropped,
            retry_note.as_deref(),
        ));
        conclude(shared, held, operation, None, successors).await
    }
}

/// When the retry this turn earns is due, or `None` when it earns none: a failed turn retries
/// after `FAILED_RETRY` unless the streak is degraded or the limit of failed turns is reached,
/// and a skipped turn retries at the time it names.
fn retry_due(
    snapshot: &OperationSnapshot,
    turn: &StepRecord,
    output: &TurnOutput,
    shared: &OpShared,
    triage: &crate::triage::Shared,
) -> Option<jiff::Timestamp> {
    // A person asked for this run: it ends with its reason instead of retrying by itself.
    if matches!(output.kind, TurnKind::Failed | TurnKind::Skipped)
        && matches!(&turn.step.intent, StepIntent::ReasoningTurn { context_refs }
            if context_refs.iter().any(|part| part == MANUAL_REF))
    {
        return None;
    }
    match output.kind {
        TurnKind::Failed => {
            let failed = snapshot
                .steps
                .iter()
                .filter(|record| record.step.kind == StepKind::ReasoningTurn)
                .filter(|record| record.progress.outcome == StepOutcome::Failed)
                .count();
            (!limits(triage).degraded && failed < FAILED_RETRY_LIMIT)
                .then(|| after(shared, FAILED_RETRY))
        }
        TurnKind::Skipped => output.retry_at,
        _ => {
            let _ = turn;
            None
        }
    }
}

/// The sentence that follows the cause of a failed or skipped turn when a retry is parked:
/// when it is due and, for a failure, which attempt it is. A skipped turn whose own reason
/// already names the time says nothing more.
fn retry_text(
    snapshot: &OperationSnapshot,
    output: &TurnOutput,
    due: jiff::Timestamp,
) -> Option<String> {
    let at = due.strftime("%H:%M UTC");
    match output.kind {
        TurnKind::Failed => {
            let failed = snapshot
                .steps
                .iter()
                .filter(|record| record.step.kind == StepKind::ReasoningTurn)
                .filter(|record| record.progress.outcome == StepOutcome::Failed)
                .count();
            Some(format!(
                "will retry at {at}, attempt {} of {}",
                failed + 1,
                FAILED_RETRY_LIMIT + 1
            ))
        }
        TurnKind::Skipped => {
            let named = output
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("tries again"));
            (!named).then(|| format!("will retry at {at}"))
        }
        _ => None,
    }
}

/// The `notify` effect that attaches the outcome.
fn notify(
    shared: &OpShared,
    operation: domovoi_core::OperationId,
    turn: StepId,
    draft: &EffectRecord,
    dropped: &[String],
    retry_note: Option<&str>,
) -> EffectSpec {
    let tools = draft
        .payload
        .get("tools")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    spec(
        EffectKind::Notify,
        json!({
            "v": 1,
            "operation": operation.to_string(),
            "step": turn.to_string(),
            "tools": tools,
            "dropped": dropped,
            "retry_note": retry_note,
        }),
        after(shared, jiff::SignedDuration::ZERO),
        NOTIFY_ATTEMPTS,
        Some(format!("notify:{turn}")),
    )
}

/// An asked turn stores the question and nothing else: no assessment, plan or notification,
/// and no notify effect. The operation waits for the answer or the timeout.
async fn ask_owner(
    shared: &Arc<OpShared>,
    held: Held,
    operation: OperationId,
    snapshot: &OperationSnapshot,
    turn: &StepRecord,
    output: &TurnOutput,
) -> EffectOutcome {
    let turn_id = turn.step.id;
    if caused_by(snapshot, turn_id, StepKind::Question).is_empty() {
        let id = StepId::new();
        let text = cut_chars(&output.headline(), 500);
        let step = NewStep {
            id,
            kind: StepKind::Question,
            phase: "ask".to_owned(),
            parent: Some(turn_id),
            intent: StepIntent::Question {
                text,
                expires_at: after(shared, shared.settings.question_timeout),
            },
        };
        let edge = NewEdge {
            from: id,
            to: turn_id,
            kind: EdgeKind::CausedBy,
        };
        if let Err(err) = handle_of(shared)
            .propose(operation, vec![step], vec![edge])
            .await
        {
            return retry(err);
        }
    }
    conclude(shared, held, operation, None, Vec::new()).await
}
