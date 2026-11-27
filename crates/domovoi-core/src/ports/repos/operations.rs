//! Reading operations. There is no write method: operations change only through the unit of
//! work, by the operation writer.

use async_trait::async_trait;
use jiff::Timestamp;

use super::{Page, Paged, StoreError};
use crate::capability::ContractId;
use crate::id::{ExecutionId, HostId, IncidentId, OperationId};
use crate::operation::{ExecutionRecord, Operation, OperationKind, OperationSnapshot};
use crate::ports::uow::Versioned;

/// Which operations a listing returns. The default returns all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperationFilter {
    /// Only operations with this status name (see
    /// [`OperationStatus::NAMES`](crate::OperationStatus::NAMES)).
    pub status: Option<String>,
    /// Only operations linked to this incident.
    pub incident: Option<IncidentId>,
    /// Only operations that pin a subject on this host.
    pub host: Option<HostId>,
    /// Only operations of this kind.
    pub kind: Option<OperationKind>,
}

/// Stored operations, read-only. Listings order by `created_at`, newest first.
#[async_trait]
pub trait OperationRepo: Send + Sync {
    /// The operation and its version.
    async fn get(&self, id: OperationId) -> Result<Option<Versioned<Operation>>, StoreError>;

    /// The operation with its steps (in ordinal order), edges, execution rows and incident
    /// links, from one consistent read.
    async fn load(&self, id: OperationId) -> Result<Option<OperationSnapshot>, StoreError>;

    /// One page of operations matching `filter`, newest first.
    async fn list(
        &self,
        filter: &OperationFilter,
        page: &Page,
    ) -> Result<Paged<Operation>, StoreError>;

    /// Every operation linked to `incident`, newest first.
    async fn for_incident(&self, incident: IncidentId) -> Result<Vec<Operation>, StoreError>;

    /// The execution row with `id` and its version.
    async fn execution(
        &self,
        id: ExecutionId,
    ) -> Result<Option<Versioned<ExecutionRecord>>, StoreError>;

    /// Executions on `host` whose step is running: handed to the agent, no result yet.
    async fn dispatched_on(&self, host: HostId) -> Result<Vec<ExecutionRecord>, StoreError>;

    /// How many executions of `contract` were dispatched to `host` at or after `since`,
    /// whatever their outcome.
    async fn dispatched_since(
        &self,
        host: HostId,
        contract: &ContractId,
        since: Timestamp,
    ) -> Result<u32, StoreError>;

    /// The executions on `host` handed to the agent at or between `from` and `to` (inclusive),
    /// whatever their outcome, oldest dispatch first. Executions never dispatched are not
    /// returned: nothing ran.
    async fn dispatched_between(
        &self,
        host: HostId,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<ExecutionRecord>, StoreError>;

    /// Up to `limit` operations whose status name is in `statuses`, oldest first.
    async fn with_status(
        &self,
        statuses: &[&str],
        limit: u16,
    ) -> Result<Vec<Operation>, StoreError>;
}
