//! The code-owned fields of a summary, derived from a finished operation's typed steps.
//!
//! Pure: reads a loaded snapshot, never the clock or a store.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::id::{HostId, StepId};
use crate::operation::{
    EdgeKind, OperationSnapshot, OperationStatus, StepIntent, StepKind, StepOutcome,
};

use super::field::SummaryField;
use super::summary::{ExecutedContract, ResolutionOutcome, SummarySubject};

/// Most executed contracts a summary lists.
const MAX_CONTRACTS: usize = 32;
/// Most hosts a summary lists.
const MAX_HOSTS: usize = 16;

/// The five code-owned fields of one finished operation.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeFields {
    /// How it ended.
    pub outcome: ResolutionOutcome,
    /// Whether the last verification passed and the execution it checked succeeded.
    pub verified: bool,
    /// The pinned subjects, in order.
    pub subjects: Vec<SummarySubject>,
    /// Executions that were actually handed to an agent, in ordinal order.
    pub contracts: Vec<ExecutedContract>,
    /// The sorted, distinct hosts of the subjects and the executions.
    pub hosts: Vec<HostId>,
}

impl CodeFields {
    /// The canonical JSON of each field, in [`SummaryField::ALL`] order.
    #[must_use]
    pub fn values(&self) -> [(SummaryField, Value); 5] {
        [
            (SummaryField::Outcome, json!(self.outcome.as_str())),
            (SummaryField::Verified, json!(self.verified)),
            (SummaryField::Subjects, json!(self.subjects)),
            (SummaryField::ContractsExecuted, json!(self.contracts)),
            (SummaryField::Hosts, json!(self.hosts)),
        ]
    }
}

/// The code-owned fields of `snapshot`; `None` while the operation is not terminal.
#[must_use]
pub fn code_fields(snapshot: &OperationSnapshot) -> Option<CodeFields> {
    let outcome = match snapshot.operation.status {
        OperationStatus::Succeeded => ResolutionOutcome::Succeeded,
        OperationStatus::Failed => ResolutionOutcome::Failed,
        OperationStatus::Escalated => ResolutionOutcome::Escalated,
        OperationStatus::Cancelled => ResolutionOutcome::Cancelled,
        _ => return None,
    };
    let subjects: Vec<SummarySubject> = snapshot
        .operation
        .pinned
        .iter()
        .map(|p| SummarySubject {
            host: p.host,
            check: p.check.clone(),
            target: p.target.clone(),
        })
        .collect();
    let mut contracts = Vec::new();
    let mut hosts: BTreeSet<HostId> = subjects.iter().map(|s| s.host).collect();
    for row in &snapshot.executions {
        hosts.insert(row.value.host);
    }
    for record in &snapshot.steps {
        if record.step.kind != StepKind::Execution || contracts.len() >= MAX_CONTRACTS {
            continue;
        }
        let Some(row) = snapshot.execution_of(record.step.id) else {
            continue;
        };
        if row.value.dispatched.is_none() {
            continue;
        }
        contracts.push(ExecutedContract {
            contract: row.value.contract.to_string(),
            host: row.value.host,
            target: row.value.target.as_ref().map(ToString::to_string),
            outcome: record.progress.outcome.as_str().to_owned(),
        });
    }
    Some(CodeFields {
        outcome,
        verified: verified(snapshot),
        subjects,
        contracts,
        hosts: hosts.into_iter().take(MAX_HOSTS).collect(),
    })
}

/// Whether the highest-ordinal verification passed and the execution it checks succeeded.
fn verified(snapshot: &OperationSnapshot) -> bool {
    let Some(last) = snapshot
        .steps
        .iter()
        .rfind(|r| r.step.kind == StepKind::Verification)
    else {
        return false;
    };
    if last.progress.outcome != StepOutcome::Succeeded {
        return false;
    }
    let from_edge = snapshot
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Verifies && e.from == last.step.id)
        .map(|e| e.to);
    let target: Option<StepId> = from_edge.or(match &last.step.intent {
        StepIntent::Verification { execution, .. } => Some(*execution),
        _ => None,
    });
    target
        .and_then(|id| snapshot.step(id))
        .is_some_and(|r| r.progress.outcome == StepOutcome::Succeeded)
}

/// A sorted, deduplicated set of outcome words from the closed vocabulary `cancelled`,
/// `denied`, `escalated`, `expired`, `outcome_unknown`, `question_asked`, `refused`,
/// `retried` and `verification_failed`. A search aid only: indexed, never shown, never a
/// revision.
#[must_use]
pub fn outcome_tags(snapshot: &OperationSnapshot) -> Vec<&'static str> {
    let mut tags = BTreeSet::new();
    let approved: BTreeSet<StepId> = snapshot
        .steps
        .iter()
        .filter_map(|r| match &r.step.intent {
            StepIntent::Approval { execution, .. } => Some(*execution),
            _ => None,
        })
        .collect();
    for record in &snapshot.steps {
        let outcome = record.progress.outcome;
        match record.step.kind {
            StepKind::Approval if outcome == StepOutcome::Refused => {
                tags.insert("denied");
            }
            StepKind::Approval if outcome == StepOutcome::NotRun => {
                tags.insert("expired");
            }
            StepKind::Execution if outcome == StepOutcome::Refused => {
                if !approved.contains(&record.step.id) {
                    tags.insert("refused");
                }
            }
            StepKind::Execution if outcome == StepOutcome::OutcomeUnknown => {
                tags.insert("outcome_unknown");
            }
            StepKind::Verification if outcome == StepOutcome::Failed => {
                tags.insert("verification_failed");
            }
            StepKind::Question => {
                tags.insert("question_asked");
            }
            _ => {}
        }
    }
    if snapshot.edges.iter().any(|e| e.kind == EdgeKind::RetryOf) {
        tags.insert("retried");
    }
    match snapshot.operation.status {
        OperationStatus::Escalated => {
            tags.insert("escalated");
        }
        OperationStatus::Cancelled => {
            tags.insert("cancelled");
        }
        _ => {}
    }
    tags.into_iter().collect()
}
