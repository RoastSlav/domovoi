//! Who authorised a run, and the packaged policy decision a later milestone will consume.
//!
//! `Authorization` is a wrapper with private internals: the two cases are not public variants,
//! so the only construction paths are [`Authorization::human`] and [`Authorization::policy`].
//! Both, and [`PolicyPermit::issue`], are confined by `xtask arch-check`. No permit is issued
//! operationally at M1: `issue` is a data factory that evaluates and grants nothing.

use jiff::Timestamp;

use super::approval::{HumanApproval, PlanHash};

/// Which case an [`Authorization`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationKind {
    /// An authenticated human approved.
    Human,
    /// A policy permit authorised, which M1 refuses at the writer.
    Policy,
}

impl AuthorizationKind {
    /// The stored name, `human` or `policy`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Policy => "policy",
        }
    }
}

/// Who authorised a run.
///
/// The representation is private, so `Authorization { .. }` and `Authorization::Human(..)`
/// cannot be built outside this module. There is no `Default`, no `Deserialize` and no
/// `From`/`Into`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Authorization {
    source: AuthorizationSource,
}

/// Private on purpose: never exported and never named outside this module.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AuthorizationSource {
    Human(HumanApproval),
    Policy(PolicyPermit),
}

impl Authorization {
    /// Wraps a human approval.
    ///
    /// Call only from `crates/domovoi-hub/src/auth/` and from tests. No authority is gained
    /// here; the approval already came from an authenticated human.
    #[must_use]
    pub fn human(approval: HumanApproval) -> Self {
        Self {
            source: AuthorizationSource::Human(approval),
        }
    }

    /// Wraps a policy permit.
    ///
    /// Call only from `crates/domovoi-hub/src/policy/` and from tests. M1 has no policy
    /// evaluation and the writer refuses this case.
    #[must_use]
    pub fn policy(permit: PolicyPermit) -> Self {
        Self {
            source: AuthorizationSource::Policy(permit),
        }
    }

    /// The human approval, when this is the human case.
    #[must_use]
    pub fn human_approval(&self) -> Option<&HumanApproval> {
        match &self.source {
            AuthorizationSource::Human(approval) => Some(approval),
            AuthorizationSource::Policy(_) => None,
        }
    }

    /// The policy permit, when this is the policy case.
    #[must_use]
    pub fn policy_permit(&self) -> Option<&PolicyPermit> {
        match &self.source {
            AuthorizationSource::Policy(permit) => Some(permit),
            AuthorizationSource::Human(_) => None,
        }
    }

    /// Which case this is. The only way to learn it; `authorization_kind` is derived here.
    #[must_use]
    pub fn kind(&self) -> AuthorizationKind {
        match self.source {
            AuthorizationSource::Human(_) => AuthorizationKind::Human,
            AuthorizationSource::Policy(_) => AuthorizationKind::Policy,
        }
    }
}

/// A packaged policy decision.
///
/// Every field is private and the only constructor is [`PolicyPermit::issue`]. It performs
/// no policy evaluation, holds no registry and grants no authority; the operational issuer
/// does not exist in M1. The three policy identifiers, `ceiling_hash` and `safety_identity`
/// are documented opaque strings until the decision types land.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PolicyPermit {
    decision_id: String,
    rule_ids: Vec<String>,
    policy_version: String,
    plan_hash: PlanHash,
    issued_at: Timestamp,
    expires_at: Timestamp,
    ceiling_hash: String,
    safety_identity: Option<String>,
}

/// The arguments of [`PolicyPermit::issue`], kept in one value so the factory stays a
/// single-argument function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyPermitParts<'a> {
    /// The decision this permit packs.
    pub decision_id: &'a str,
    /// The rules the decision applied.
    pub rule_ids: Vec<String>,
    /// The policy version the decision used.
    pub policy_version: &'a str,
    /// The plan the decision covered.
    pub plan_hash: PlanHash,
    /// When the decision was made.
    pub issued_at: Timestamp,
    /// When the permit stops being valid.
    pub expires_at: Timestamp,
    /// The autonomy ceiling the decision stayed under, as an opaque value.
    pub ceiling_hash: &'a str,
    /// The safety identity a containment decision binds, as an opaque value.
    pub safety_identity: Option<&'a str>,
}

impl PolicyPermit {
    /// Packs the data a policy decision would carry.
    ///
    /// Call only from `crates/domovoi-hub/src/policy/` and from tests. This is a data
    /// factory, **not** a permit issuer: it evaluates nothing. `rule_ids` is sorted and
    /// deduplicated so equal decisions have one representation.
    pub fn issue(parts: PolicyPermitParts<'_>) -> Result<Self, PermitError> {
        let PolicyPermitParts {
            decision_id,
            rule_ids,
            policy_version,
            plan_hash,
            issued_at,
            expires_at,
            ceiling_hash,
            safety_identity,
        } = parts;
        if decision_id.is_empty() {
            return Err(PermitError::EmptyDecisionId);
        }
        if policy_version.is_empty() {
            return Err(PermitError::EmptyPolicyVersion);
        }
        if rule_ids.iter().any(String::is_empty) {
            return Err(PermitError::EmptyRuleId);
        }
        if expires_at <= issued_at {
            return Err(PermitError::ExpiresNotAfterIssued);
        }
        let mut rule_ids = rule_ids;
        rule_ids.sort();
        rule_ids.dedup();
        Ok(Self {
            decision_id: decision_id.to_owned(),
            rule_ids,
            policy_version: policy_version.to_owned(),
            plan_hash,
            issued_at,
            expires_at,
            ceiling_hash: ceiling_hash.to_owned(),
            safety_identity: safety_identity.map(str::to_owned),
        })
    }

    /// The plan the decision covered.
    #[must_use]
    pub fn plan_hash(&self) -> PlanHash {
        self.plan_hash
    }

    /// When the permit stops being valid.
    #[must_use]
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    /// The decision this permit packs.
    #[must_use]
    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }
}

/// Why [`PolicyPermit::issue`] refused its own arguments.
///
/// It says nothing about policy and nothing about authority.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermitError {
    /// The decision id is empty.
    #[error("a policy decision needs a decision id")]
    EmptyDecisionId,
    /// The policy version is empty.
    #[error("a policy decision needs a policy version")]
    EmptyPolicyVersion,
    /// A rule id is empty.
    #[error("a rule id must not be empty")]
    EmptyRuleId,
    /// The expiry is not after the issue time.
    #[error("a permit must expire after it is issued")]
    ExpiresNotAfterIssued,
}
