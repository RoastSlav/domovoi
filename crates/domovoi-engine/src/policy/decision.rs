//! The decision and its trace.

use std::collections::BTreeSet;

use domovoi_core::capability::{FactId, TargetName};
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::condition::{Condition, FactStatus};
use super::outcome::{ConfirmationRequirement, HardLimit, Outcome, OutcomeReason};
use super::request::ManagementPath;
use super::rule::RuleId;

/// The constraints an `Allow` carries forward to the permit.
///
/// They are intersections: when several equal winners allow, the permit may do only what all
/// of them allow.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Constraints {
    /// The lowest `MaxRate` bound among the winners, further reduced by the requested rate.
    pub max_per_hour: Option<u32>,
    /// The lowest `MaxTtl` bound among the winners, further reduced by the requested TTL.
    pub max_ttl: Option<SignedDuration>,
    /// The intersection of the winners' `Exact` sets; `None` only when every winner is `Any`.
    pub targets: Option<BTreeSet<TargetName>>,
    /// The conjunction of the winners' conditions, in winner id order then each rule's order.
    pub conditions: Vec<Condition>,
}

/// A broader rule that a more specific winner overrode.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Overridden {
    /// The rule that lost, because a more specific rule matched.
    pub broader: RuleId,
    /// The rule that won.
    pub by: RuleId,
}

/// Something the UI must highlight.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Warning {
    /// A specific `Allow` overrode a broader `Deny`: a deliberate exception that an editor
    /// mistake or a compromised session would also produce, so it is shown as dangerous.
    DangerousException {
        /// The specific `Allow`.
        allow: RuleId,
        /// The broader `Deny` it overrode.
        deny: RuleId,
    },
    /// The helper's safety set could not be verified.
    SafetySetUnverified,
    /// These management paths are discovered but not protected.
    ManagementPathsUnprotected {
        /// The unprotected paths, sorted.
        paths: Vec<ManagementPath>,
    },
}

/// The complete, deterministic trace of one evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// What the policy says.
    pub outcome: Outcome,
    /// How the resolution concluded. A malformed input is an `Err`, never a `Decision`.
    pub reason: OutcomeReason,
    /// Every matching rule of the selected set, sorted by id.
    pub matched: Vec<RuleId>,
    /// The maximal matching rules, sorted by id.
    pub winners: Vec<RuleId>,
    /// Every matching locked rule, sorted by id. Non-empty implies `LockedDeny`.
    pub locked: Vec<RuleId>,
    /// Broader rules beaten by a more specific winner, sorted by `(broader, by)`. Recorded for
    /// every strict domination, whether or not the outcomes match.
    pub overridden: Vec<Overridden>,
    /// The ordinary resolution with no hard limits, present when the final outcome differs
    /// from it, and always present for a locked `Deny`.
    pub would_have_been: Option<Box<Decision>>,
    /// The constraints an `Allow` carries.
    pub constraints: Constraints,
    /// The hard limits that applied, in declaration order.
    pub hard_limits_applied: Vec<HardLimit>,
    /// Every fact the resolution consulted, sorted by id. The status only, never the value.
    pub facts_used: Vec<(FactId, FactStatus)>,
    /// Whether the owner must type a confirmation to approve.
    pub typed_confirmation: ConfirmationRequirement,
    /// Stable, sorted warnings.
    pub warnings: Vec<Warning>,
    /// The policy version tag, echoed.
    pub policy_version: Option<Uuid>,
    /// The ceiling summary's hash, echoed for change detection.
    pub ceiling_hash: Option<String>,
}
