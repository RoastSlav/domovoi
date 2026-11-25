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
                intent.contract == planned.contract
                    && intent.host == planned.host
                    && intent.check == planned.check
                    && intent.target == planned.target
                    && intent.args == planned.args
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domovoi_core::capability::{ContractVersion, TargetIdentity};
    use domovoi_core::{
        Budgets, CheckKey, ContractId, HostId, Operation, OperationKind, PinnedSubject, SkillName,
        Step, StepIntent, StepProgress, StepRecord, TargetKey,
    };
    use jiff::Timestamp;

    use super::*;

    /// What a stored or planned step runs.
    #[derive(Clone)]
    struct Run {
        host: HostId,
        check: &'static str,
        target: Option<&'static str>,
    }

    fn intent_of(run: &Run) -> ExecutionIntent {
        ExecutionIntent {
            contract: ContractId::skill(&SkillName::parse("restart_container").expect("a name")),
            contract_version: 1,
            host: run.host,
            check: CheckKey::parse(run.check).expect("a check key"),
            target: run
                .target
                .map(|t| TargetKey::parse(t).expect("a target key")),
            args: BTreeMap::from([("container".to_owned(), "nginx".to_owned())]),
            retry_of: None,
        }
    }

    fn planned(run: &Run) -> PlannedStep {
        let intent = intent_of(run);
        PlannedStep {
            input: PlanStepInput {
                contract: intent.contract.to_string(),
                args: BTreeMap::new(),
                host: None,
                target: None,
                reason: "It is stuck.".to_owned(),
                retry_of: None,
            },
            contract: intent.contract,
            version: ContractVersion::new(1).expect("a version"),
            host: intent.host,
            check: intent.check,
            target: intent.target,
            args: intent.args,
            reason: "It is stuck.".to_owned(),
            identity: TargetIdentity::parse("container:nginx").expect("an identity"),
            retry_of: None,
            note: None,
            unit: None,
        }
    }

    /// A snapshot whose turn stored one Execution step for each of `stored`.
    fn snapshot_with(turn: StepId, stored: &[Run]) -> OperationSnapshot {
        let subject = PinnedSubject {
            host: HostId::new(),
            check: CheckKey::parse("disk.usage").expect("a check key"),
            target: None,
        };
        let operation = Operation::queued(
            OperationId::new(),
            OperationKind::Investigate,
            "Investigate: disk",
            vec![subject],
            None,
            Budgets::new(Default::default()),
            Timestamp::UNIX_EPOCH,
        )
        .expect("an operation");
        let steps = stored
            .iter()
            .enumerate()
            .map(|(ordinal, run)| {
                let intent = StepIntent::Execution(intent_of(run));
                StepRecord {
                    step: Step {
                        id: StepId::new(),
                        operation: operation.id,
                        ordinal: u32::try_from(ordinal).expect("few steps"),
                        kind: intent.kind(),
                        phase: "p".to_owned(),
                        parent: Some(turn),
                        cycle: 0,
                        intent,
                        created_at: Timestamp::UNIX_EPOCH,
                    },
                    progress: StepProgress::pending(),
                }
            })
            .collect();
        OperationSnapshot {
            operation,
            version: 1,
            steps,
            edges: Vec::new(),
            executions: Vec::new(),
            incidents: Vec::new(),
        }
    }

    #[test]
    fn a_stored_step_is_recognised_only_for_the_same_pinned_subject() {
        let turn = StepId::new();
        let host = HostId::new();
        let first = Run {
            host,
            check: "docker.container",
            target: Some("nginx"),
        };
        let snapshot = snapshot_with(turn, std::slice::from_ref(&first));

        assert!(
            stored(&snapshot, turn, &planned(&first)),
            "a redelivery keeps the stored step"
        );
        let other_target = Run {
            target: Some("redis"),
            ..first.clone()
        };
        let other_check = Run {
            check: "docker.health",
            ..first.clone()
        };
        let other_host = Run {
            host: HostId::new(),
            ..first.clone()
        };
        for different in [other_target, other_check, other_host] {
            assert!(
                !stored(&snapshot, turn, &planned(&different)),
                "same contract and arguments, different subject: still to be stored"
            );
        }
        assert!(
            !stored(&snapshot, StepId::new(), &planned(&first)),
            "another turn's step is not this turn's"
        );
    }
}
