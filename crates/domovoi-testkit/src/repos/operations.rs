//! The in-memory operation repository: reads only. Writes arrive through the unit of work.

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateRef, OperationFilter, OperationRepo, Page, Paged, StoreError, Versioned,
};
use domovoi_core::{
    ContractId, ExecutionId, ExecutionRecord, HostId, IncidentId, Operation, OperationId,
    OperationSnapshot, StepOutcome,
};
use jiff::Timestamp;

use super::{InMemoryStore, State, limit, paginate, version};

/// The stored outcome of the step an execution belongs to.
fn step_outcome(state: &State, execution: &ExecutionRecord) -> Option<StepOutcome> {
    state
        .op_steps
        .get(&execution.operation)?
        .iter()
        .find(|record| record.step.id == execution.step)
        .map(|record| record.progress.outcome)
}

/// Whether `filter` lets `operation` through.
fn matches(state: &State, filter: &OperationFilter, operation: &Operation) -> bool {
    filter
        .status
        .as_deref()
        .is_none_or(|status| operation.status.name() == status)
        && filter.incident.is_none_or(|incident| {
            state
                .op_incidents
                .get(&operation.id)
                .is_some_and(|linked| linked.contains(&incident))
        })
        && filter
            .host
            .is_none_or(|host| operation.pinned.iter().any(|p| p.host == host))
        && filter.kind.is_none_or(|kind| operation.kind == kind)
}

#[async_trait]
impl OperationRepo for InMemoryStore {
    async fn get(&self, id: OperationId) -> Result<Option<Versioned<Operation>>, StoreError> {
        let state = self.begin()?;
        Ok(state.operations.get(&id).map(|operation| Versioned {
            value: operation.clone(),
            version: version(&state, &AggregateRef::operation(id)),
        }))
    }

    async fn load(&self, id: OperationId) -> Result<Option<OperationSnapshot>, StoreError> {
        let state = self.begin()?;
        let Some(operation) = state.operations.get(&id) else {
            return Ok(None);
        };
        let steps = state.op_steps.get(&id).cloned().unwrap_or_default();
        let executions = steps
            .iter()
            .filter_map(|record| state.executions.get(&record.step.id))
            .map(|(execution, version)| Versioned {
                value: execution.clone(),
                version: *version,
            })
            .collect();
        Ok(Some(OperationSnapshot {
            operation: operation.clone(),
            version: version(&state, &AggregateRef::operation(id)),
            steps,
            edges: state.op_edges.get(&id).cloned().unwrap_or_default(),
            executions,
            incidents: state
                .op_incidents
                .get(&id)
                .map(|linked| linked.iter().copied().collect())
                .unwrap_or_default(),
        }))
    }

    async fn list(
        &self,
        filter: &OperationFilter,
        page: &Page,
    ) -> Result<Paged<Operation>, StoreError> {
        let state = self.begin()?;
        let items = state
            .operations
            .values()
            .filter(|operation| matches(&state, filter, operation))
            .map(|operation| {
                (
                    (
                        operation.created_at.as_nanosecond(),
                        operation.id.to_string(),
                    ),
                    operation.clone(),
                )
            })
            .collect();
        paginate(items, page)
    }

    async fn for_incident(&self, incident: IncidentId) -> Result<Vec<Operation>, StoreError> {
        let state = self.begin()?;
        let mut operations: Vec<Operation> = state
            .operations
            .values()
            .filter(|operation| {
                state
                    .op_incidents
                    .get(&operation.id)
                    .is_some_and(|linked| linked.contains(&incident))
            })
            .cloned()
            .collect();
        operations.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.to_string().cmp(&a.id.to_string()))
        });
        Ok(operations)
    }

    async fn execution(
        &self,
        id: ExecutionId,
    ) -> Result<Option<Versioned<ExecutionRecord>>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .executions
            .get(&id.as_step())
            .map(|(execution, version)| Versioned {
                value: execution.clone(),
                version: *version,
            }))
    }

    async fn dispatched_on(&self, host: HostId) -> Result<Vec<ExecutionRecord>, StoreError> {
        let state = self.begin()?;
        let mut running: Vec<ExecutionRecord> = state
            .executions
            .values()
            .map(|(execution, _)| execution)
            .filter(|execution| {
                execution.host == host
                    && step_outcome(&state, execution) == Some(StepOutcome::Running)
            })
            .cloned()
            .collect();
        running.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.execution.to_string().cmp(&b.execution.to_string()))
        });
        Ok(running)
    }

    async fn dispatched_since(
        &self,
        host: HostId,
        contract: &ContractId,
        since: Timestamp,
    ) -> Result<u32, StoreError> {
        let state = self.begin()?;
        let count = state
            .executions
            .values()
            .map(|(execution, _)| execution)
            .filter(|execution| {
                execution.host == host
                    && execution.contract == *contract
                    && execution.dispatched.is_some_and(|(_, at)| at >= since)
            })
            .count();
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    async fn with_status(&self, statuses: &[&str], max: u16) -> Result<Vec<Operation>, StoreError> {
        let state = self.begin()?;
        let mut operations: Vec<Operation> = state
            .operations
            .values()
            .filter(|operation| statuses.contains(&operation.status.name()))
            .cloned()
            .collect();
        operations.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.to_string().cmp(&b.id.to_string()))
        });
        operations.truncate(limit(max));
        Ok(operations)
    }
}
