//! `DraftSummary`'s second look at the model's suggested steps: they are validated again
//! against the operation as it stands now, and only the accepted ones become Execution steps.

use std::sync::Arc;

use domovoi_core::meta::{PlanInput, PlanStepInput};
use domovoi_core::{
    ExecutionIntent, OperationId, OperationSnapshot, StepId, StepKind, TriageReport,
};

use super::plumbing::handle_of;
use super::turn_output::{TurnKind, TurnOutput, caused_by};
use crate::effects::EffectOutcome;
use crate::operations::OperationError;
use crate::operations::effects::load;
use crate::operations::handle::ExecutionRequest;
use crate::operations::writer::OpShared;
use crate::plan::{PlanScope, PlanValidator, PlannedStep};
use crate::triage::Shared;

/// The steps to check for `output`: the ones the loop staged, or, for a cached report or a
/// turn stored without a plan, the report's own proposals.
fn input_of(output: &TurnOutput, report: &TriageReport) -> PlanInput {
    if output.kind == TurnKind::Report && !output.plan.is_empty() {
        let steps: Vec<PlanStepInput> = output.plan.clone();
        return PlanInput { steps };
    }
    PlanInput::from_proposals(&report.proposals).unwrap_or(PlanInput { steps: Vec::new() })
}

/// The scope to check `turn`'s steps in: the Execution steps the turn stored before (a
/// redelivery) are neither prior runs nor spent budget, so they are not refused as repeats.
fn scope_for(snapshot: &OperationSnapshot, turn: StepId) -> PlanScope {
    let mut scope = PlanScope::for_operation(snapshot);
    let own: Vec<StepId> = caused_by(snapshot, turn, StepKind::Execution)
        .iter()
        .map(|record| record.step.id)
        .collect();
    scope.prior.retain(|run| !own.contains(&run.step));
    if let Some(left) = scope.executions_left.as_mut() {
        let given_back = u32::try_from(own.len()).unwrap_or(u32::MAX);
        *left = left.saturating_add(given_back);
    }
    scope
}

/// Whether an Execution step the turn caused already runs `planned`, so a redelivery keeps it
/// instead of proposing it twice.
fn stored(snapshot: &OperationSnapshot, turn: StepId, planned: &PlannedStep) -> bool {
    caused_by(snapshot, turn, StepKind::Execution)
        .iter()
        .any(|record| match &record.step.intent {
            domovoi_core::StepIntent::Execution(intent) => {
                intent.contract == planned.contract && intent.args == planned.args
            }
            _ => false,
        })
}

/// Validates the turn's steps once more and stores the accepted ones through
/// `propose_execution`. Returns the lines of the steps that were refused, by the validator or
/// by the writer.
pub(super) async fn store_plan(
    shared: &Arc<OpShared>,
    triage: &Shared,
    operation: OperationId,
    turn: StepId,
    output: &TurnOutput,
    report: &TriageReport,
) -> Result<Vec<String>, EffectOutcome> {
    let input = input_of(output, report);
    if input.steps.is_empty() {
        return Ok(Vec::new());
    }
    let snapshot = load(shared, operation).await?;
    let deps = &triage.deps;
    let validator = PlanValidator {
        registry: deps.capabilities.as_ref(),
        resolver: deps.resolver.as_ref(),
        hosts: deps.hosts.as_ref(),
        redactor: deps.redactor.as_ref(),
    };
    let verdict = validator
        .validate(&scope_for(&snapshot, turn), &[], &input)
        .await;
    let mut dropped: Vec<String> = verdict
        .refused
        .iter()
        .map(|refusal| format!("{}: {}", short_name(&refusal.contract), refusal.message))
        .collect();
    let handle = handle_of(shared);
    for planned in verdict.accepted {
        if stored(&snapshot, turn, &planned) {
            continue;
        }
        let name = short_name(planned.contract.as_str()).to_owned();
        let request = ExecutionRequest {
            intent: ExecutionIntent {
                contract: planned.contract,
                contract_version: planned.version.get(),
                host: planned.host,
                check: planned.check,
                target: planned.target,
                args: planned.args,
                retry_of: planned.retry_of,
            },
            parent: Some(turn),
            caused_by: Some(turn),
        };
        match handle.propose_execution(operation, request).await {
            Ok(_) => {}
            Err(OperationError::HostUnavailable(_)) => dropped.push(format!(
                "{name}: no authenticated agent session to bind the action to"
            )),
            Err(err) => {
                return Err(EffectOutcome::Retry {
                    error: err.to_string(),
                });
            }
        }
    }
    Ok(dropped)
}

/// The skill name of a `skill.<name>` contract id, else the id itself.
fn short_name(contract: &str) -> &str {
    contract.strip_prefix("skill.").unwrap_or(contract)
}
