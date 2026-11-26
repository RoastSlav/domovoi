//! Reading the hub's record of containment blocks. There is no write method: every change
//! goes through the unit of work as a `StateWrite::Containment`.

use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::json;

use super::StoreError;
use crate::actor::Actor;
use crate::id::{ExecutionId, HostId, OperationId, StepId};
use crate::ports::{AggregateRef, JournalDraft, JournalKind};

/// Where a block is in its life. Stored as the word [`BlockPhase::as_str`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPhase {
    /// Approved, not yet dispatched.
    Queued,
    /// The `apply` request is committed and may be in flight.
    Applying,
    /// `apply` succeeded; the kernel's own timeout is running.
    Applied,
    /// The `confirm` request is committed and may be in flight.
    Confirming,
    /// `confirm` succeeded; the block lasts until its deadline.
    Confirmed,
    /// The `rollback` request is committed and may be in flight.
    RollingBack,
    /// The block was removed by a `rollback`.
    RolledBack,
    /// The kernel removed the block when its timeout ran out.
    Expired,
    /// The firewall table disappeared with the block in it.
    Lost,
    /// `apply` was refused or never sent; nothing is in the kernel.
    NotRun,
    /// A phase failed in a way that is not one of the other end states.
    Failed,
    /// A phase was sent and its result is unknown until the next reconcile.
    OutcomeUnknown,
}

impl BlockPhase {
    /// Every phase, for tests and listings.
    pub const ALL: [Self; 12] = [
        Self::Queued,
        Self::Applying,
        Self::Applied,
        Self::Confirming,
        Self::Confirmed,
        Self::RollingBack,
        Self::RolledBack,
        Self::Expired,
        Self::Lost,
        Self::NotRun,
        Self::Failed,
        Self::OutcomeUnknown,
    ];

    /// The phases whose result a reconcile after a reconnect asks the agent about.
    pub const RECONCILABLE: [Self; 4] = [
        Self::Applying,
        Self::Applied,
        Self::Confirming,
        Self::RollingBack,
    ];

    /// The stored word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Applying => "applying",
            Self::Applied => "applied",
            Self::Confirming => "confirming",
            Self::Confirmed => "confirmed",
            Self::RollingBack => "rolling_back",
            Self::RolledBack => "rolled_back",
            Self::Expired => "expired",
            Self::Lost => "lost",
            Self::NotRun => "not_run",
            Self::Failed => "failed",
            Self::OutcomeUnknown => "outcome_unknown",
        }
    }

    /// Reads [`BlockPhase::as_str`] back; `None` for any other text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|phase| phase.as_str() == text)
    }

    /// Whether the block has reached an end state that nothing changes any more. A
    /// `Confirmed` block ends by expiry, rollback or loss, so it is not final yet.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(
            self,
            Self::RolledBack | Self::Expired | Self::Lost | Self::NotRun | Self::Failed
        )
    }
}

/// One stored block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRow {
    /// The execution id of the `apply`; names the block in every phase.
    pub block_id: ExecutionId,
    /// The host the block is on.
    pub host_id: HostId,
    /// The operation whose step asked for it.
    pub operation_id: OperationId,
    /// The step.
    pub step_id: StepId,
    /// The canonical network text of the blocked source.
    pub source: String,
    /// How long the block lasts after `confirm`, in seconds.
    pub ttl_s: u32,
    /// The plan hash all three phases carry, as hex.
    pub plan_hash: String,
    /// Where the block is.
    pub phase: BlockPhase,
    /// `human` or `policy`.
    pub authorization_kind: String,
    /// JSON: what the approval bound about safety (verified identity, or unverified reason
    /// and unprotected paths).
    pub assurance: String,
    /// The execution id of the `confirm`, once sent.
    pub confirm_execution: Option<ExecutionId>,
    /// The execution id of the `rollback`, once sent.
    pub rollback_execution: Option<ExecutionId>,
    /// When the hub saw the `apply` result.
    pub applied_at: Option<Timestamp>,
    /// When a confirmed block ends.
    pub deadline: Option<Timestamp>,
    /// What the helper said about flows that existed before the block.
    pub existing_flows: Option<String>,
    /// Whether the helper reported a bridge-family coverage gap.
    pub bridge_gap: Option<bool>,
    /// The token of the refusal or failure that ended the block.
    pub failure_token: Option<String>,
    /// The detail that came with the token.
    pub failure_message: Option<String>,
    /// When the row was created.
    pub created_at: Timestamp,
    /// When the row last changed.
    pub updated_at: Timestamp,
    /// Bumped by the store on every change; guards [`PhaseGuard`].
    pub version: u64,
}

/// What a stored block must still look like for a phase change to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseGuard {
    /// The phase the caller read.
    pub phase: BlockPhase,
    /// The version the caller read.
    pub version: u64,
}

/// The change one phase step makes. `Some` fields replace the stored value; `None` keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseUpdate {
    /// The phase to move to.
    pub phase: BlockPhase,
    /// Set when `confirm` is committed as `Confirming`.
    pub confirm_execution: Option<ExecutionId>,
    /// Set when `rollback` is committed as `RollingBack`.
    pub rollback_execution: Option<ExecutionId>,
    /// Set when the `apply` result arrives.
    pub applied_at: Option<Timestamp>,
    /// Set when `confirm` succeeds.
    pub deadline: Option<Timestamp>,
    /// What the helper reported about earlier flows.
    pub existing_flows: Option<String>,
    /// What the helper reported about the bridge gap.
    pub bridge_gap: Option<bool>,
    /// The refusal or failure token.
    pub failure_token: Option<String>,
    /// The detail of the refusal or failure.
    pub failure_message: Option<String>,
    /// When the change happened.
    pub at: Timestamp,
}

impl PhaseUpdate {
    /// A move to `phase` at `at` that changes nothing else.
    #[must_use]
    pub const fn to(phase: BlockPhase, at: Timestamp) -> Self {
        Self {
            phase,
            confirm_execution: None,
            rollback_execution: None,
            applied_at: None,
            deadline: None,
            existing_flows: None,
            bridge_gap: None,
            failure_token: None,
            failure_message: None,
            at,
        }
    }
}

/// The journal row for one phase change: payload `{block, host, from, to, token?}`.
#[must_use]
pub fn phase_journal(
    block: &BlockRow,
    from: BlockPhase,
    to: BlockPhase,
    token: Option<&str>,
    actor: Actor,
    at: Timestamp,
) -> JournalDraft {
    let mut payload = json!({
        "block": block.block_id.to_string(),
        "host": block.host_id.to_string(),
        "from": from.as_str(),
        "to": to.as_str(),
    });
    if let (Some(token), Some(map)) = (token, payload.as_object_mut()) {
        map.insert("token".to_owned(), json!(token));
    }
    JournalDraft {
        aggregate: AggregateRef::containment_block(block.block_id),
        kind: JournalKind::Containment,
        payload_v: 1,
        payload,
        actor,
        occurred_at: at,
    }
}

/// Read access to containment blocks.
#[async_trait]
pub trait ContainmentRepo: Send + Sync {
    /// One block by the execution id of its `apply`.
    async fn get_block(&self, block: ExecutionId) -> Result<Option<BlockRow>, StoreError>;

    /// The block a step created, if any.
    async fn block_for_step(
        &self,
        operation: OperationId,
        step: StepId,
    ) -> Result<Option<BlockRow>, StoreError>;

    /// A host's blocks, newest first, at most `limit`.
    async fn blocks_for_host(&self, host: HostId, limit: u32) -> Result<Vec<BlockRow>, StoreError>;

    /// A host's blocks in a [`BlockPhase::RECONCILABLE`] phase, oldest first.
    async fn reconcilable_blocks(&self, host: HostId) -> Result<Vec<BlockRow>, StoreError>;
}
