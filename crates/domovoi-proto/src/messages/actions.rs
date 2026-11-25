//! Capability execution, refusals and the action journal's status query.

use std::collections::BTreeMap;

use domovoi_core::attest::ExpectedSafety;
use domovoi_core::{PlanHash, TargetKey};
use jiff::Timestamp;
use uuid::Uuid;

use crate::Open;

/// Run one approved capability and reply `capability_result` before the agent's own timeout.
///
/// The wire carries the primitive execution id; `domovoi-core`'s `ExecutionId` arrives with a
/// later change and converts at its own boundary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunCapability {
    /// The wire form of the execution id.
    pub execution_id: Uuid,
    /// Contract id, `<family>.<verb>`, for example `container.restart`.
    pub contract: String,
    /// Contract version, strictly increasing per contract.
    pub version: u32,
    /// Canonical target, for example `container:plex`.
    pub target: TargetKey,
    /// Canonical argument map; values are canonical strings.
    pub args: BTreeMap<String, String>,
    /// The plan hash the approval bound to.
    pub plan_hash: PlanHash,
    /// Whether a human or a policy authorizes the run.
    pub authorization_kind: AuthorizationKind,
    /// When the authorization stops being usable.
    pub expires_at: Timestamp,
    /// Required for `contain`, refused when missing or malformed.
    #[serde(default)]
    pub expected_safety: Option<ExpectedSafety>,
}

/// Who authorized a capability run.
///
/// Closed: it decides authority, so a value this build does not know is malformed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationKind {
    /// An authenticated human approved the run.
    Human,
    /// A policy approved the run.
    Policy,
}

/// The answer to `run_capability`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CapabilityResult {
    /// What happened.
    pub outcome: CapabilityOutcome,
}

/// A capability run's output or the reason it failed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityOutcome {
    /// The capability ran.
    Succeeded {
        /// Per-contract outputs as canonical strings, keyed by the contract.
        outputs: BTreeMap<String, String>,
    },
    /// The capability did not complete.
    Failed {
        /// The failure class; an unknown value is preserved and treated as failure.
        code: Open<CapabilityFailureCode>,
        /// A short, redacted explanation.
        message: String,
    },
}

/// A capability failure class this build knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityFailureCode {
    /// The backend could not start the work.
    SpawnFailed,
    /// The work passed its timeout.
    TimedOut,
    /// The work exited non-zero.
    ExitedNonZero,
    /// The output was larger than the contract allows.
    OutputTooLarge,
    /// The helper or backend is not available.
    HelperUnavailable,
    /// A backend refused after accepting the work.
    Refused,
}

/// An execution the agent refused before accepting it.
///
/// A refusal ends that execution and never ends the session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Refused {
    /// Why; an unknown value is preserved for the human reading it.
    pub code: Open<RefusalCode>,
    /// Human-readable, short, already redacted.
    pub detail: String,
    /// Seconds until a retry may succeed.
    #[serde(default)]
    pub retry_after: Option<u32>,
    /// Present for a rate-limit refusal, naming the binding limit.
    #[serde(default)]
    pub scope: Option<RefusalScope>,
}

/// A refusal class this build knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    /// The mutation rate limiter would be exceeded.
    RateLimited,
    /// The unattended ceiling would be exceeded.
    CeilingExceeded,
    /// The agent's policy does not allow the risk.
    AllowRiskDenied,
    /// The plan hash differs from the one the agent recomputed.
    PlanHashMismatch,
    /// The target changed since the plan was made.
    TargetChanged,
    /// The authorization expired.
    AuthorizationExpired,
    /// A policy authorization needs a verified safety claim.
    PolicyRequiresVerifiedSafety,
    /// The agent does not know the contract.
    ContractUnknown,
    /// The agent does not support the contract version.
    ContractVersionUnsupported,
    /// The action journal is corrupt; only a local owner reset recovers.
    JournalCorrupt,
}

/// Which limit a rate-limit refusal binds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalScope {
    /// The global per-hour limit.
    Global,
    /// One contract's per-hour limit.
    Capability {
        /// The contract id the limit applies to.
        contract: String,
    },
}

/// Ask what happened to a set of executions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActionStatusQuery {
    /// The executions to ask about; at most 64.
    pub execution_ids: Vec<Uuid>,
    /// The epoch the hub believes it is asking about.
    pub expected_epoch: Uuid,
}

/// The journal's answers, one per requested id, in request order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActionStatusAnswers {
    /// The journal's actual epoch.
    pub epoch: Uuid,
    /// One answer per requested id.
    pub answers: Vec<ActionStatusAnswer>,
}

/// What the journal knows about one execution.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActionStatusAnswer {
    /// The execution asked about.
    pub execution_id: Uuid,
    /// What the journal knows.
    pub state: ActionState,
}

/// What the journal knows about one execution.
///
/// Closed: it decides state, so a value this build does not know is malformed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    /// The execution completed and its result is durable.
    Finished {
        /// The durable result.
        result: CapabilityResult,
    },
    /// This agent instance started the execution and still holds it.
    Running,
    /// The request never reached this journal.
    NotFound,
    /// The outcome cannot be known.
    OutcomeUnknown {
        /// Why; an unknown value is preserved.
        reason: Open<OutcomeUnknownReason>,
    },
}

/// Why an outcome is unknowable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeUnknownReason {
    /// The journal recorded acceptance but never a start record.
    NoStartRecord,
    /// The agent restarted while the execution was recorded as started.
    AgentRestarted,
    /// The journal lost the record, or the hub's epoch is stale.
    JournalLost,
}
