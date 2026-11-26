//! What a triage run proposed, seen as the Execution steps of an investigation, so one
//! scorer and one set of probes judge it.
//!
//! Included by the test binaries that score proposals.

use std::collections::BTreeMap;

use domovoi_core::{OperationSnapshot, Risk, SkillSummary, StepKind};

/// One proposed run of a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalView {
    /// The skill's name.
    pub skill: String,
    /// The arguments.
    pub args: BTreeMap<String, String>,
    /// The skill's risk.
    pub risk: Risk,
    /// Whether it still waits for a person.
    pub awaiting_human: bool,
    /// Whether a person approved it.
    pub approved: bool,
}

/// The views of the Execution steps of `snapshot` that have an executions row, so a refused
/// proposal, which never became anything a person could approve, is not one. `skills` give
/// each skill's risk.
pub fn from_snapshot(snapshot: &OperationSnapshot, skills: &[SkillSummary]) -> Vec<ProposalView> {
    snapshot
        .steps
        .iter()
        .filter(|record| record.step.kind == StepKind::Execution)
        .filter_map(|record| {
            let row = &snapshot.execution_of(record.step.id)?.value;
            let skill = row.contract.skill_name()?;
            let risk = skills
                .iter()
                .find(|summary| summary.name == skill)
                .map_or(Risk::Destructive, |summary| summary.risk);
            Some(ProposalView {
                skill: skill.to_string(),
                args: row.args.clone(),
                risk,
                awaiting_human: row.approval().is_none() && !record.progress.outcome.is_final(),
                approved: row.approval().is_some(),
            })
        })
        .collect()
}
