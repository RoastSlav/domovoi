//! Reading permits, the pause switch, circuit-breaker inputs and unresolved unknown outcomes.
//! There is no write method: every change goes through the unit of work as a
//! `StateWrite::Permit`.

use async_trait::async_trait;
use jiff::Timestamp;

use super::StoreError;
use crate::capability::ContractId;
use crate::id::{ExecutionId, HostId, StepId};
use crate::signal::TargetKey;

/// The single autonomy row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyRow {
    /// Whether new permits and undispatched permits are blocked.
    pub paused: bool,
    /// Who last changed it.
    pub changed_by: Option<String>,
    /// When it last changed.
    pub changed_at: Option<Timestamp>,
    /// Why it was paused.
    pub reason: Option<String>,
}

/// A rule with failures counted inside the breaker window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFailures {
    /// The rule id.
    pub rule: String,
    /// How many permit-authorized executions naming the rule ended `Failed` or
    /// `OutcomeUnknown` after the later of the window start and the rule's last reset.
    pub failures: u32,
    /// When the earliest counted failure settled.
    pub since: Timestamp,
}

/// An execution whose outcome is unknown and which nobody has resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedUnknown {
    /// The execution.
    pub execution: ExecutionId,
    /// Its step.
    pub step: StepId,
}

/// Read access to permits and autonomy state.
#[async_trait]
pub trait PermitRepo: Send + Sync {
    /// The pause switch.
    async fn autonomy(&self) -> Result<AutonomyRow, StoreError>;

    /// Permits issued at or after `since`, optionally for one host and contract.
    async fn permits_issued_since(
        &self,
        since: Timestamp,
        scope: Option<(HostId, &ContractId)>,
    ) -> Result<u32, StoreError>;

    /// Permits issued at or after `since` that name `rule`.
    async fn rule_permits_since(&self, rule: &str, since: Timestamp) -> Result<u32, StoreError>;

    /// Rules with at least one counted failure, computed from the executions and the reset rows
    /// (never from a stored flag).
    async fn rule_failures(&self, window_start: Timestamp)
    -> Result<Vec<RuleFailures>, StoreError>;

    /// An unresolved `OutcomeUnknown` execution of `contract` on `target` at `host`, if any.
    /// A configuration change does not clear it: the key is the target, not the identity hash.
    async fn unresolved_unknown(
        &self,
        host: HostId,
        contract: &ContractId,
        target: &TargetKey,
    ) -> Result<Option<UnresolvedUnknown>, StoreError>;
}
