//! `DraftSummary`: the assessment, plan, execution proposals and notification of a finished
//! turn, and the retry a skipped or failed turn earns.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord};
use domovoi_core::{
    ContractId, EdgeKind, ExecutionIntent, FinishOutcome, OperationSnapshot, StepId, StepIntent,
    StepKind, StepOutcome, StepRecord,
};
use serde_json::{Value, json};

use super::plumbing::cut as cut_chars;
use super::plumbing::{
    claim, conclude, end_operation, handle_of, incident_of, latest_turn, runtime, step_of,
};
use super::turn_output::{TurnKind, TurnOutput, caused_by, output_of};
use super::{BUILD_ATTEMPTS, NOTIFY_ATTEMPTS};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::OperationError;
use crate::operations::decide::{EffectSpec, NewEdge, NewStep};
use crate::operations::effects::{Address, address, after, finish, load, spec, unreadable};
use crate::operations::handle::ExecutionRequest;
use crate::operations::writer::OpShared;
use crate::triage::proposals::check;
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
        let incident = match incident_of(&runtime, &snapshot).await {
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
        let triage = &runtime.triage;
        let handle = handle_of(shared);
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
            let skills = triage.deps.capabilities.enabled_skills(incident.host());
            for proposal in &report.proposals {
                let (skill, args) = match check(proposal, &incident, &skills) {
                    Ok(found) => found,
                    Err(why) => {
                        dropped.push(
                            triage
                                .deps
                                .redactor
                                .redact(&format!("{}: {why}", proposal.skill)),
                        );
                        continue;
                    }
                };
                let args: BTreeMap<String, String> = args
                    .iter()
                    .map(|(name, value)| (name.to_owned(), value.to_owned()))
                    .collect();
                let contract = ContractId::skill(&skill.name);
                if already_proposed(&snapshot, turn_id, &contract, &args) {
                    continue;
                }
                let request = ExecutionRequest {
                    intent: ExecutionIntent {
                        contract,
                        contract_version: 1,
                        host: incident.host(),
                        check: incident.check().clone(),
                        target: incident.target().cloned(),
                        args,
                        retry_of: None,
                    },
                    parent: Some(turn_id),
                    caused_by: Some(turn_id),
                };
                match handle.propose_execution(operation, request).await {
                    Ok(_) => {}
                    Err(OperationError::HostUnavailable(_)) => dropped.push(format!(
                        "{}: no authenticated agent session to bind the action to",
                        skill.name
                    )),
                    Err(err) => return retry(err),
                }
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
        if let Some(due) = retry_due(&snapshot, &turn, &output, shared, triage) {
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
        successors.push(notify(shared, operation, turn_id, effect, &dropped));
        conclude(shared, held, operation, None, successors).await
    }
}

/// Whether an Execution step the turn caused already runs the same contract with the same
/// arguments, so a redelivery keeps it instead of proposing it twice.
fn already_proposed(
    snapshot: &OperationSnapshot,
    turn: StepId,
    contract: &ContractId,
    args: &BTreeMap<String, String>,
) -> bool {
    caused_by(snapshot, turn, StepKind::Execution)
        .iter()
        .any(|record| match &record.step.intent {
            StepIntent::Execution(intent) => &intent.contract == contract && &intent.args == args,
            _ => false,
        })
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

/// The `notify` effect that attaches the outcome.
fn notify(
    shared: &OpShared,
    operation: domovoi_core::OperationId,
    turn: StepId,
    draft: &EffectRecord,
    dropped: &[String],
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
        }),
        after(shared, jiff::SignedDuration::ZERO),
        NOTIFY_ATTEMPTS,
        Some(format!("notify:{turn}")),
    )
}
