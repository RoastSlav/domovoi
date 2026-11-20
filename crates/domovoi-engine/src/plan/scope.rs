//! What a plan may touch and what the operation already did.

use std::collections::BTreeMap;

use domovoi_core::{
    CheckKey, ContractId, HostId, Incident, OperationId, OperationSnapshot, PinnedSubject, StepId,
    StepIntent, StepOutcome, TargetKey,
};

/// One Execution step of the operation, as the validator needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorRun {
    /// The step.
    pub step: StepId,
    /// The capability it ran.
    pub contract: ContractId,
    /// The host.
    pub host: HostId,
    /// The check.
    pub check: CheckKey,
    /// The target.
    pub target: Option<TargetKey>,
    /// The arguments.
    pub args: BTreeMap<String, String>,
    /// How it stands.
    pub outcome: StepOutcome,
}

/// What a plan is checked against.
#[derive(Debug, Clone)]
pub struct PlanScope {
    /// The operation, when there is one.
    pub operation: Option<OperationId>,
    /// What the plan may act on, 1 to 8 subjects.
    pub pinned: Vec<PinnedSubject>,
    /// Executions the budget still allows; `None` is unlimited.
    pub executions_left: Option<u32>,
    /// The operation's Execution steps so far.
    pub prior: Vec<PriorRun>,
    /// The current cycle.
    pub cycle: u32,
}

impl PlanScope {
    /// The scope of an incident with no operation: one subject, no budget, no earlier runs.
    #[must_use]
    pub fn for_incident(incident: &Incident) -> Self {
        Self {
            operation: None,
            pinned: vec![PinnedSubject {
                host: incident.host(),
                check: incident.check().clone(),
                target: incident.target().cloned(),
            }],
            executions_left: None,
            prior: Vec::new(),
            cycle: 0,
        }
    }

    /// The scope of a loaded operation.
    #[must_use]
    pub fn for_operation(snapshot: &OperationSnapshot) -> Self {
        let operation = &snapshot.operation;
        let prior = snapshot
            .steps
            .iter()
            .filter_map(|record| match &record.step.intent {
                StepIntent::Execution(intent) => Some(PriorRun {
                    step: record.step.id,
                    contract: intent.contract.clone(),
                    host: intent.host,
                    check: intent.check.clone(),
                    target: intent.target.clone(),
                    args: intent.args.clone(),
                    outcome: record.progress.outcome,
                }),
                _ => None,
            })
            .collect();
        Self {
            operation: Some(operation.id),
            pinned: operation.pinned.clone(),
            executions_left: operation
                .budgets
                .limits
                .executions
                .map(|limit| limit.saturating_sub(operation.budgets.used.executions)),
            prior,
            cycle: operation.cycle,
        }
    }
}
