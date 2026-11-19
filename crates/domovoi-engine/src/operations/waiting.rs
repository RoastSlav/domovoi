//! The Execution steps of an incident that are waiting for a person, as a card shows them.

use domovoi_core::ports::{OperationRepo, StoreError};
use domovoi_core::{ExecutionState, HostName, Incident};

use super::state::OperationState;
use crate::cards::WaitingStep;

/// The incident's executions awaiting approval on the incident's host, oldest first, at most
/// `limit`. Reads only. `host` must be the name of the incident's host: it is shown on the
/// card as given. An expired step is settled by its wake effect, so it is listed here
/// until then, as a waiting action is.
///
/// # Errors
/// Returns the store's error when an operation cannot be read.
pub async fn waiting_steps(
    repo: &dyn OperationRepo,
    incident: &Incident,
    host: &HostName,
    limit: usize,
) -> Result<Vec<WaitingStep>, StoreError> {
    let mut steps = Vec::new();
    for operation in repo.for_incident(incident.id()).await? {
        if operation.status.is_terminal() {
            continue;
        }
        let Some(snapshot) = repo.load(operation.id).await? else {
            continue;
        };
        let state = OperationState::new(snapshot);
        for row in &state.snapshot().executions {
            let record = &row.value;
            if record.host != incident.host()
                || state.execution_state(record.step) != Some(ExecutionState::AwaitingApproval)
            {
                continue;
            }
            let Some(skill) = record.contract.skill_name() else {
                continue;
            };
            steps.push(WaitingStep {
                execution: record.execution,
                skill: skill.to_string(),
                host: host.clone(),
                risk: record.risk(),
                args: record
                    .args
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
                plan_short: record.plan_hash.short(),
                plan_prefix: hex::encode(&record.plan_hash.as_bytes()[..3]),
                expires_at: record.expires_at,
                created_at: record.created_at,
            });
        }
    }
    steps.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.execution.cmp(&b.execution))
    });
    steps.truncate(limit);
    Ok(steps)
}
