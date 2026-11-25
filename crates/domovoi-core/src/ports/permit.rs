//! The port that turns an enforced policy `Allow` into a stored permit.
//!
//! The hub implements it in `crates/domovoi-hub/src/policy/`; the engine's permit effect calls
//! it and never builds an authorization itself. The engine's evaluator outcome is carried by
//! name, because core cannot name the engine's type.

use async_trait::async_trait;

use crate::action::Authorization;
use crate::id::StepId;
use crate::operation::{ExecutionRecord, PinnedSubject};
use crate::unit::UnitRefusal;

/// What a permit rate limit counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateScope {
    /// The rule's own `max_per_hour`.
    Rule,
    /// The host and contract.
    HostContract,
    /// `[policy] max_permits_per_hour`, for the whole hub.
    Global,
}

/// Why a permit was withheld although the evaluator may have allowed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermitRefusal {
    /// The contract is not in the closed policy-dispatchable list.
    NotPolicyDispatchable,
    /// The autonomy switch is paused.
    Paused,
    /// A rule the decision names is suspended by its circuit breaker.
    RuleSuspended(String),
    /// An earlier restart of this unit has an unresolved unknown outcome.
    UnresolvedUnknown(StepId),
    /// A permit rate limit is spent.
    RateLimited(RateScope),
    /// The plan hash recomputed now differs from the stored one.
    PlanChanged,
    /// The helper refused the unit at re-resolution.
    IdentityRefused(UnitRefusal),
    /// No policy version is published.
    NoPublishedPolicy,
    /// The inputs the evaluator needs could not be read.
    InputsUnavailable,
}

impl PermitRefusal {
    /// The stable token written to the audit detail.
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::NotPolicyDispatchable => "not_policy_dispatchable".to_owned(),
            Self::Paused => "paused".to_owned(),
            Self::RuleSuspended(rule) => format!("rule_suspended:{rule}"),
            Self::UnresolvedUnknown(step) => format!("unresolved_unknown:{step}"),
            Self::RateLimited(RateScope::Rule) => "rate_limited:rule".to_owned(),
            Self::RateLimited(RateScope::HostContract) => "rate_limited:host_contract".to_owned(),
            Self::RateLimited(RateScope::Global) => "rate_limited:global".to_owned(),
            Self::PlanChanged => "plan_changed".to_owned(),
            Self::IdentityRefused(refusal) => format!("identity_refused:{}", refusal.token()),
            Self::NoPublishedPolicy => "no_published_policy".to_owned(),
            Self::InputsUnavailable => "inputs_unavailable".to_owned(),
        }
    }

    /// Whether the refusal is a safety guard whose withholding is audited (`paused`,
    /// `rule_suspended`, `unresolved_unknown`, `rate_limited`), to bound audit volume.
    #[must_use]
    pub fn is_audited(&self) -> bool {
        matches!(
            self,
            Self::Paused
                | Self::RuleSuspended(_)
                | Self::UnresolvedUnknown(_)
                | Self::RateLimited(_)
        )
    }
}

/// One execution step to consider for a permit.
#[derive(Debug, Clone)]
pub struct PermitRequest {
    /// The proposed execution.
    pub execution: ExecutionRecord,
    /// The operation's pinned subjects.
    pub pinned: Vec<PinnedSubject>,
}

/// A permit built from an enforced `Allow`, with the decision document it came from.
#[derive(Debug, Clone)]
pub struct PermitGrant {
    /// The policy authorization to record on the execution.
    pub authorization: Authorization,
    /// The enforced decision document, as text.
    pub decision: String,
}

/// Why no permit was issued; the step stays an approval offer.
#[derive(Debug, Clone)]
pub enum NotIssued {
    /// The evaluator did not allow.
    Evaluated {
        /// The evaluator's outcome by name: `require_approval`, `deny` or `unavailable`.
        outcome: String,
        /// The decision document, as text.
        decision: String,
    },
    /// A guard withheld the permit.
    Withheld(PermitRefusal),
}

/// The issuer's answer.
#[derive(Debug, Clone)]
pub enum PermitOutcome {
    /// A permit was issued.
    Issued(PermitGrant),
    /// No permit was issued.
    NotIssued(NotIssued),
}

/// Issues policy permits. The only implementation calls `PolicyPermit::issue` and
/// `Authorization::policy`; nothing else may.
#[async_trait]
pub trait PermitIssuer: Send + Sync {
    /// Considers `request` and issues a permit only when every guard holds.
    async fn issue(&self, request: PermitRequest) -> PermitOutcome;
}
