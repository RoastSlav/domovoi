//! Why a rule set, a request or a publication review was refused.

use domovoi_core::capability::{ContractCategory, ContractId};

use super::rule::RuleId;

/// Why a rule set, a request or a publication review was refused. Never carries a secret,
/// never carries owner text.
///
/// An `Err` produces no decision, so a caller has nothing it could act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// A rule is `locked` but is not a `deny` rule.
    #[error("rule {rule}: `locked` is only valid on a `deny` rule, found {found}")]
    LockedNotDeny {
        /// The offending rule.
        rule: RuleId,
        /// The outcome the rule has.
        found: &'static str,
    },
    /// A rule's own outcome is `unavailable`, which only a hard limit can produce.
    #[error("rule {rule}: a rule outcome cannot be `unavailable`")]
    UnavailableRuleOutcome {
        /// The offending rule.
        rule: RuleId,
    },
    /// Two rules share an id.
    #[error("duplicate rule id: {0}")]
    DuplicateRuleId(RuleId),
    /// A rule names an empty set of targets.
    #[error("rule {rule}: `targets: Exact(..)` is empty; write `Any` instead")]
    EmptyTargetSet {
        /// The offending rule.
        rule: RuleId,
    },
    /// A time window that can never match.
    #[error("rule {rule}: the time window {start}..{end} is empty")]
    DegenerateTimeWindow {
        /// The offending rule.
        rule: RuleId,
        /// The window start, `HH:MM`.
        start: String,
        /// The window end, `HH:MM`.
        end: String,
    },
    /// An argument condition names an argument that cannot exist.
    #[error("rule {rule}: {key:?} is not a valid argument name")]
    BadArgKey {
        /// The offending rule.
        rule: RuleId,
        /// The rejected key.
        key: String,
    },
    /// The contract index has no such contract.
    #[error("unknown contract id: {0}")]
    UnknownContract(ContractId),
    /// The contract index has no contract in this category.
    #[error("category {0} is not in the contract index")]
    CategoryNotInIndex(ContractCategory),
    /// The contract exists but has no category and effect on record.
    #[error("contract {0} is not indexed with a category and effect")]
    ContractNotIndexed(ContractId),
    /// A containment posture was supplied for a contract that does not contain.
    #[error("a containment posture was supplied for a contract that does not contain")]
    ContainmentOnNonContain,
    /// A ceiling hash was expected but no ceiling summary was supplied.
    #[error("a ceiling hash was recorded but no ceiling summary was supplied")]
    CeilingHashMissing,
    /// More facts than a trace may carry.
    #[error("{count} facts were supplied; at most {max} are accepted")]
    TooManyFacts {
        /// How many facts were supplied.
        count: usize,
        /// The most that are accepted.
        max: usize,
    },
}
