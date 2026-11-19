//! The stored form of a decision. [`Decision`] is not serializable and carries owner text in
//! its error type, so the recorder keeps its own mirror documents.

use std::collections::BTreeSet;

use domovoi_core::capability::{FactId, TargetName};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::policy::{
    Condition, ConfirmationRequirement, Decision, FactStatus, HardLimit, Outcome, OutcomeReason,
    Overridden, PolicyError, RuleId, Warning,
};

/// The envelope version this build writes and reads.
pub(super) const VERSION: u64 = 1;

/// One fact the resolution consulted, with its status only. The observed value is never
/// recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactUse {
    /// The fact.
    pub fact: FactId,
    /// Known, unknown, stale or disputed.
    pub status: FactStatus,
}

/// The constraints an `Allow` carries, in plain numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintsDoc {
    /// The lowest rate bound.
    pub max_per_hour: Option<u32>,
    /// The lowest TTL bound, in seconds.
    pub max_ttl_secs: Option<i64>,
    /// The intersection of the winners' target sets.
    pub targets: Option<BTreeSet<TargetName>>,
    /// The conjunction of the winners' conditions.
    pub conditions: Vec<Condition>,
}

/// A [`Decision`] in a form that serializes and replays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionDoc {
    /// What the policy says.
    pub outcome: Outcome,
    /// How the resolution concluded.
    pub reason: OutcomeReason,
    /// Every matching rule, sorted by id.
    pub matched: Vec<RuleId>,
    /// The maximal matching rules.
    pub winners: Vec<RuleId>,
    /// Every matching locked rule.
    pub locked: Vec<RuleId>,
    /// Broader rules beaten by a more specific winner.
    pub overridden: Vec<Overridden>,
    /// The ordinary resolution when hard limits changed it.
    pub would_have_been: Option<Box<DecisionDoc>>,
    /// The constraints an `Allow` carries.
    pub constraints: ConstraintsDoc,
    /// The hard limits that applied.
    pub hard_limits_applied: Vec<HardLimit>,
    /// The facts consulted.
    pub facts_used: Vec<FactUse>,
    /// Whether a typed confirmation is required.
    pub typed_confirmation: ConfirmationRequirement,
    /// Warnings.
    pub warnings: Vec<Warning>,
    /// The policy version tag.
    pub policy_version: Option<Uuid>,
    /// The ceiling summary's hash.
    pub ceiling_hash: Option<String>,
}

impl DecisionDoc {
    /// Copies `decision` field for field. The decision is already sorted by the evaluator's
    /// determinism rules, and the order is kept.
    #[must_use]
    pub fn from_decision(decision: &Decision) -> Self {
        Self {
            outcome: decision.outcome,
            reason: decision.reason,
            matched: decision.matched.clone(),
            winners: decision.winners.clone(),
            locked: decision.locked.clone(),
            overridden: decision.overridden.clone(),
            would_have_been: decision
                .would_have_been
                .as_deref()
                .map(|d| Box::new(Self::from_decision(d))),
            constraints: ConstraintsDoc {
                max_per_hour: decision.constraints.max_per_hour,
                max_ttl_secs: decision.constraints.max_ttl.map(|d| d.as_secs()),
                targets: decision.constraints.targets.clone(),
                conditions: decision.constraints.conditions.clone(),
            },
            hard_limits_applied: decision.hard_limits_applied.clone(),
            facts_used: decision
                .facts_used
                .iter()
                .map(|(fact, status)| FactUse {
                    fact: fact.clone(),
                    status: *status,
                })
                .collect(),
            typed_confirmation: decision.typed_confirmation,
            warnings: decision.warnings.clone(),
            policy_version: decision.policy_version,
            ceiling_hash: decision.ceiling_hash.clone(),
        }
    }
}

/// What one evaluation stored: a decision, or the code of the error that refused it.
#[derive(Debug, Clone, PartialEq)]
pub enum DecisionEnvelope {
    /// The evaluator produced a decision.
    Decision(Box<DecisionDoc>),
    /// The evaluator refused; only the code is kept, never owner text.
    Error {
        /// A snake_case error code.
        code: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Repr {
    v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decision: Option<DecisionDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl DecisionEnvelope {
    /// The envelope for an evaluator result.
    #[must_use]
    pub fn from_result(result: &Result<Decision, PolicyError>) -> Self {
        match result {
            Ok(decision) => Self::Decision(Box::new(DecisionDoc::from_decision(decision))),
            Err(error) => Self::Error {
                code: error_code(error),
            },
        }
    }

    /// `{"v":1,"decision":..}` or `{"v":1,"error":".."}`.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error when the document cannot be written.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let repr = match self {
            Self::Decision(doc) => Repr {
                v: VERSION,
                decision: Some(DecisionDoc::clone(doc)),
                error: None,
            },
            Self::Error { code } => Repr {
                v: VERSION,
                decision: None,
                error: Some(code.clone()),
            },
        };
        serde_json::to_string(&repr)
    }

    /// Reads [`DecisionEnvelope::to_json`] back; `None` for any other shape or version.
    #[must_use]
    pub fn from_json(text: &str) -> Option<Self> {
        let repr: Repr = serde_json::from_str(text).ok()?;
        if repr.v != VERSION {
            return None;
        }
        match (repr.decision, repr.error) {
            (Some(doc), None) => Some(Self::Decision(Box::new(doc))),
            (None, Some(code)) => Some(Self::Error { code }),
            _ => None,
        }
    }

    /// The outcome word stored in the row's outcome column.
    #[must_use]
    pub fn outcome_text(&self) -> &'static str {
        match self {
            Self::Decision(doc) => outcome_text(doc.outcome),
            Self::Error { .. } => "error",
        }
    }
}

/// The word the outcome columns use for `outcome`.
#[must_use]
pub const fn outcome_text(outcome: Outcome) -> &'static str {
    outcome.name()
}

/// The error's variant name in snake_case. Never owner text.
#[must_use]
pub fn error_code(error: &PolicyError) -> String {
    match error {
        PolicyError::LockedNotDeny { .. } => "locked_not_deny",
        PolicyError::UnavailableRuleOutcome { .. } => "unavailable_rule_outcome",
        PolicyError::DuplicateRuleId(_) => "duplicate_rule_id",
        PolicyError::EmptyTargetSet { .. } => "empty_target_set",
        PolicyError::DegenerateTimeWindow { .. } => "degenerate_time_window",
        PolicyError::InvalidTimeOfDay { .. } => "invalid_time_of_day",
        PolicyError::NegativeMaxTtl { .. } => "negative_max_ttl",
        PolicyError::BadArgKey { .. } => "bad_arg_key",
        PolicyError::UnknownContract(_) => "unknown_contract",
        PolicyError::CategoryNotInIndex(_) => "category_not_in_index",
        PolicyError::ContractNotIndexed(_) => "contract_not_indexed",
        PolicyError::ContainmentOnNonContain => "containment_on_non_contain",
        PolicyError::CeilingHashMissing => "ceiling_hash_missing",
        PolicyError::TooManyFacts { .. } => "too_many_facts",
    }
    .to_owned()
}
