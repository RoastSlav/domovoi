//! The in-memory twin of the permit tables: the pause switch, resets, resolutions, and the
//! permit queries computed from the stored executions, with the same semantics as SQLite.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::ports::{
    AutonomyRow, CommitError, PermitRepo, PermitWrite, RuleFailures, StoreError, UnresolvedUnknown,
};
use domovoi_core::{ContractId, ExecutionId, HostId, StepOutcome, TargetKey};
use jiff::Timestamp;

use super::{InMemoryStore, State};

/// The autonomy row, resets and resolutions.
#[derive(Clone)]
pub(super) struct PermitState {
    autonomy: AutonomyRow,
    resets: Vec<(String, Timestamp)>,
    resolved: BTreeMap<ExecutionId, String>,
}

impl Default for PermitState {
    fn default() -> Self {
        Self {
            autonomy: AutonomyRow {
                paused: false,
                changed_by: None,
                changed_at: None,
                reason: None,
            },
            resets: Vec::new(),
            resolved: BTreeMap::new(),
        }
    }
}

/// Stores one autonomy write.
pub(super) fn apply(state: &mut State, write: &PermitWrite) -> Result<(), CommitError> {
    let permit = &mut state.permit;
    match write {
        PermitWrite::Pause { by, reason, at } => {
            permit.autonomy = AutonomyRow {
                paused: true,
                changed_by: Some(by.clone()),
                changed_at: Some(*at),
                reason: Some(reason.clone()),
            };
        }
        PermitWrite::Resume { by, at } => {
            permit.autonomy = AutonomyRow {
                paused: false,
                changed_by: Some(by.clone()),
                changed_at: Some(*at),
                reason: None,
            };
        }
        PermitWrite::ResetRule { rule, at, .. } => {
            if permit.resets.iter().any(|(r, t)| r == rule && t == at) {
                return Err(CommitError::Constraint("duplicate reset".to_owned()));
            }
            permit.resets.push((rule.clone(), *at));
        }
        PermitWrite::ResolveUnknown { execution, by, .. } => {
            if permit.resolved.contains_key(execution) {
                return Err(CommitError::Constraint(
                    "this unknown outcome was already resolved".to_owned(),
                ));
            }
            permit.resolved.insert(*execution, by.clone());
        }
    }
    Ok(())
}

fn outcome(state: &State, execution: &domovoi_core::ExecutionRecord) -> Option<StepOutcome> {
    state
        .op_steps
        .get(&execution.operation)?
        .iter()
        .find(|record| record.step.id == execution.step)
        .map(|record| record.progress.outcome)
}

#[async_trait]
impl PermitRepo for InMemoryStore {
    async fn autonomy(&self) -> Result<AutonomyRow, StoreError> {
        Ok(self.begin()?.permit.autonomy.clone())
    }

    async fn permits_issued_since(
        &self,
        since: Timestamp,
        scope: Option<(HostId, &ContractId)>,
    ) -> Result<u32, StoreError> {
        let state = self.begin()?;
        let count = state
            .executions
            .values()
            .filter_map(|(e, _)| e.permit().map(|p| (e, p.permit())))
            .filter(|(e, p)| {
                p.issued_at() >= since
                    && scope
                        .is_none_or(|(host, contract)| e.host == host && &e.contract == contract)
            })
            .count();
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    async fn rule_permits_since(&self, rule: &str, since: Timestamp) -> Result<u32, StoreError> {
        let state = self.begin()?;
        let count = state
            .executions
            .values()
            .filter_map(|(e, _)| e.permit())
            .filter(|p| p.permit().issued_at() >= since)
            .filter(|p| p.permit().rule_ids().iter().any(|r| r == rule))
            .count();
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    async fn rule_failures(
        &self,
        window_start: Timestamp,
    ) -> Result<Vec<RuleFailures>, StoreError> {
        let state = self.begin()?;
        let mut by_rule: BTreeMap<String, (u32, Timestamp)> = BTreeMap::new();
        for (execution, _) in state.executions.values() {
            let Some(stored) = execution.permit() else {
                continue;
            };
            if !matches!(
                outcome(&state, execution),
                Some(StepOutcome::Failed | StepOutcome::OutcomeUnknown)
            ) {
                continue;
            }
            let Some(settled) = execution.settled_at else {
                continue;
            };
            for rule in stored.permit().rule_ids() {
                let reset = state
                    .permit
                    .resets
                    .iter()
                    .filter(|(r, _)| r == rule)
                    .map(|(_, t)| *t)
                    .max();
                let floor = reset.map_or(window_start, |t| t.max(window_start));
                if settled >= floor {
                    let entry = by_rule.entry(rule.clone()).or_insert((0, settled));
                    entry.0 += 1;
                    entry.1 = entry.1.min(settled);
                }
            }
        }
        Ok(by_rule
            .into_iter()
            .map(|(rule, (failures, since))| RuleFailures {
                rule,
                failures,
                since,
            })
            .collect())
    }

    async fn unresolved_unknown(
        &self,
        host: HostId,
        contract: &ContractId,
        target: &TargetKey,
    ) -> Result<Option<UnresolvedUnknown>, StoreError> {
        let state = self.begin()?;
        let mut found: Option<&domovoi_core::ExecutionRecord> = None;
        for (execution, _) in state.executions.values() {
            if execution.host == host
                && &execution.contract == contract
                && execution.target.as_ref() == Some(target)
                && outcome(&state, execution) == Some(StepOutcome::OutcomeUnknown)
                && !state.permit.resolved.contains_key(&execution.execution)
                && found.is_none_or(|f| execution.created_at > f.created_at)
            {
                found = Some(execution);
            }
        }
        Ok(found.map(|e| UnresolvedUnknown {
            execution: e.execution,
            step: e.step,
        }))
    }
}
