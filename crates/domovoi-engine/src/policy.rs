//! Two unrelated things live here: the incident manager's tunables ([`EnginePolicy`]) and the
//! capability policy evaluator ([`Rule`], [`Decision`], [`evaluate`]).
//!
//! The evaluator is pure and decides what authorizes what; it adds nothing to the incident
//! manager, performs no I/O, reads no clock and never constructs an approval, a permit or an
//! authorization.

mod condition;
mod decision;
mod error;
mod evaluate;
mod limits;
mod outcome;
mod publish;
mod request;
mod rule;
mod simulate;
mod specificity;

pub use condition::{Condition, FactState, FactStatus, TimeOfDay};
pub use decision::{Constraints, Decision, Overridden, Warning};
pub use error::PolicyError;
pub use evaluate::{RuleSelection, evaluate, evaluate_shadow, validate_rules};
pub use outcome::{ConfirmationRequirement, HardLimit, Outcome, OutcomeReason, UnavailableReason};
pub use publish::{Finding, FindingKind, PublicationReport, validate_publication};
pub use request::{
    AutonomyState, CanonicalArgs, ContainmentInput, EvaluateRequest, MAX_FACTS, ManagementPath,
    names_to_resolve, resolve_contract,
};
pub use rule::{CapabilityScope, HostScope, Rule, RuleId, TargetScope};
pub use simulate::simulate;

use domovoi_core::IncidentPolicy;
use jiff::SignedDuration;

/// How the incident manager confirms, groups and expires incidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnginePolicy {
    /// The state machine's confirmation runs, reopen window and TTL.
    pub incident: IncidentPolicy,
    /// How recently an incident from another source must have opened for a signal about
    /// the same target to join it.
    pub grouping_window: SignedDuration,
    /// How often the manager ticks, expiring mutes and resolving quiet incidents.
    pub tick_every: SignedDuration,
}

impl Default for EnginePolicy {
    fn default() -> Self {
        Self {
            incident: IncidentPolicy::default(),
            grouping_window: SignedDuration::from_mins(5),
            tick_every: SignedDuration::from_secs(30),
        }
    }
}
