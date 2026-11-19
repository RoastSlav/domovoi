//! Recorded shadow decisions and the queries over them.

use std::collections::BTreeMap;

use jiff::Timestamp;

use crate::capability::ContractId;
use crate::id::{ExecutionId, HostId, OperationId};
use crate::operation::PinnedSubject;
use crate::signal::{CheckKey, TargetKey};

/// How a step's decision row came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowStatus {
    /// Both decisions were computed.
    Evaluated,
    /// The inputs could not be established.
    Unevaluable,
    /// The contract is never evaluated.
    Skipped,
}

impl ShadowStatus {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Evaluated => "evaluated",
            Self::Unevaluable => "unevaluable",
            Self::Skipped => "skipped",
        }
    }

    /// Reads [`ShadowStatus::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "evaluated" => Some(Self::Evaluated),
            "unevaluable" => Some(Self::Unevaluable),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

/// One Execution step waiting for its decision row.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowCandidate {
    /// The step's execution id.
    pub execution: ExecutionId,
    /// The operation it belongs to.
    pub operation: OperationId,
    /// The host it runs on.
    pub host: HostId,
    /// The contract it runs.
    pub contract: ContractId,
    /// The contract version it was planned against.
    pub contract_version: u32,
    /// The check it is about.
    pub check: CheckKey,
    /// The target it is about.
    pub target: Option<TargetKey>,
    /// Its arguments.
    pub args: BTreeMap<String, String>,
    /// The subjects its operation pinned.
    pub pinned: Vec<PinnedSubject>,
    /// When the step was created.
    pub created_at: Timestamp,
}

/// One decision row. The JSON columns are opaque to core: the engine owns their formats.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowRecord {
    /// The step's execution id.
    pub execution: ExecutionId,
    /// The operation it belongs to.
    pub operation: OperationId,
    /// The host it runs on.
    pub host: HostId,
    /// The contract id text.
    pub contract: String,
    /// When the step was created.
    pub step_created_at: Timestamp,
    /// When the decision was computed.
    pub evaluated_at: Timestamp,
    /// How the row came out.
    pub status: ShadowStatus,
    /// A snake_case code, set for unevaluable and skipped rows and for late ones.
    pub reason: Option<String>,
    /// The published version evaluated; 0 when none existed.
    pub policy_version: u32,
    /// The hash of the rule set evaluated.
    pub rules_hash: String,
    /// `allow`, `require_approval`, `unavailable`, `deny` or `error`.
    pub enforced_outcome: Option<String>,
    /// The same vocabulary, for the shadow rules.
    pub shadow_outcome: Option<String>,
    /// Whether any input was unavailable.
    pub degraded: bool,
    /// The candidate document (JSON).
    pub candidate: String,
    /// The recorded inputs (JSON); set only when evaluated.
    pub inputs: Option<String>,
    /// The enforced-rules decision (JSON).
    pub enforced: Option<String>,
    /// The shadow-rules decision (JSON).
    pub shadow: Option<String>,
    /// The input gaps (JSON array).
    pub gaps: String,
}

/// Counts of one recorder batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepStats {
    /// Rows with status evaluated.
    pub evaluated: u32,
    /// Rows with status unevaluable.
    pub unevaluable: u32,
    /// Rows with status skipped.
    pub skipped: u32,
}

/// The recorder's bookkeeping row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShadowState {
    /// When the last successful batch committed.
    pub last_sweep_at: Option<Timestamp>,
    /// Why the last sweep failed, until a batch succeeds.
    pub last_error: Option<String>,
    /// Total evaluated rows inserted.
    pub total_evaluated: u64,
    /// Total unevaluable rows inserted.
    pub total_unevaluable: u64,
    /// Total skipped rows inserted.
    pub total_skipped: u64,
}

/// Which decision rows a listing returns. The default returns all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShadowFilter {
    /// Only this host.
    pub host: Option<HostId>,
    /// Only this contract.
    pub contract: Option<String>,
    /// Only this status.
    pub status: Option<ShadowStatus>,
    /// Only this enforced outcome.
    pub enforced_outcome: Option<String>,
    /// Only rows whose enforced and shadow outcomes differ.
    pub differing: bool,
    /// Only rows evaluated at or after this time.
    pub since: Option<Timestamp>,
    /// Only rows evaluated before this time.
    pub until: Option<Timestamp>,
}

/// Counts of the decision rows since a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowSummary {
    /// The window start.
    pub since: Timestamp,
    /// Rows in the window.
    pub total: u64,
    /// Rows per status name.
    pub by_status: BTreeMap<String, u64>,
    /// Rows per enforced outcome.
    pub by_enforced_outcome: BTreeMap<String, u64>,
    /// Rows with a gap.
    pub degraded: u64,
    /// Rows whose enforced and shadow outcomes differ.
    pub differing: u64,
}
