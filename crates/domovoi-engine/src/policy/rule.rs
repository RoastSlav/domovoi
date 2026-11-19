//! The owner-authored rule and its three scope dimensions.

use std::collections::BTreeSet;
use std::fmt;

use domovoi_core::HostId;
use domovoi_core::ValidationError;
use domovoi_core::capability::{ContractCategory, ContractId, EffectClass, TargetName};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::condition::Condition;
use super::outcome::Outcome;

/// A stable identifier for one rule. Owner-visible; it sorts the trace and never decides
/// anything.
///
/// `[A-Za-z0-9][A-Za-z0-9_.:-]{0,63}`, case-sensitive, compared byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RuleId(String);

impl RuleId {
    /// Validates `input` as a rule id.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        let bytes = input.as_bytes();
        if bytes.is_empty() || bytes.len() > 64 {
            return Err(ValidationError::new(
                "rule id",
                "must be 1 to 64 characters",
            ));
        }
        if !bytes[0].is_ascii_alphanumeric() {
            return Err(ValidationError::new(
                "rule id",
                "must start with a letter or digit",
            ));
        }
        let ok = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-');
        if !bytes.iter().all(ok) {
            return Err(ValidationError::new(
                "rule id",
                "may contain only letters, digits, underscore, dot, colon and dash",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RuleId {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<RuleId> for String {
    fn from(value: RuleId) -> Self {
        value.0
    }
}

/// The host dimension. `All` is the whole dimension; `Host(h)` is one host.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostScope {
    /// Every host.
    All,
    /// One host.
    Host(HostId),
}

/// The capability dimension, widest first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CapabilityScope {
    /// Every capability.
    Any,
    /// Every capability of one effect class.
    Effect(EffectClass),
    /// Every capability of one category.
    Category(ContractCategory),
    /// One contract.
    Contract(ContractId),
}

/// The target dimension. `Exact` is a non-empty set of owner-typed names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "names", rename_all = "snake_case")]
pub enum TargetScope {
    /// Every target.
    Any,
    /// Only the named targets. An empty set is refused by validation.
    Exact(BTreeSet<TargetName>),
}

/// One owner-authored rule. Plain data with no methods: comparison and matching live in
/// free functions with one implementation each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// The rule's stable id.
    pub id: RuleId,
    /// A disabled rule is ignored entirely: it neither matches nor explains.
    pub enabled: bool,
    /// Shadow rules are evaluated by the shadow entry point and never enforced.
    pub shadow: bool,
    /// A hard prohibition. Valid only with `outcome: Deny`: only a prohibition can be
    /// absolute, so a lock on anything else is refused rather than guessed at.
    pub locked: bool,
    /// The host dimension.
    pub host: HostScope,
    /// The capability dimension.
    pub capability: CapabilityScope,
    /// The target dimension.
    pub targets: TargetScope,
    /// Conditions that decide whether the rule matches. They never affect specificity.
    pub conditions: Vec<Condition>,
    /// What the rule says.
    pub outcome: Outcome,
    /// Owner text shown in the editor and the trace. Never read by the algorithm.
    pub note: String,
    /// Opaque tag of the policy version this rule belongs to.
    pub version_id: Option<Uuid>,
}
