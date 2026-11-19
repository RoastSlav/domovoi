//! Everything the evaluator needs, supplied as data.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::HostId;
use domovoi_core::ValidationError;
use domovoi_core::capability::{
    CeilingSummary, ContractId, ContractIndex, ContractMeta, ContractVersion, FactId,
    TargetIdentity, TargetName,
};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::condition::FactState;
use super::error::PolicyError;
use super::rule::{Rule, TargetScope};

/// The most facts a request may carry, so a caller cannot make the trace arbitrarily large.
pub const MAX_FACTS: usize = 1024;

/// Canonical arguments, as produced by the plan builder: one `name:byte_len:value\n` line per
/// argument, in the same grammar as the validated-argument canonical form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalArgs(String);

impl CanonicalArgs {
    /// Holds the text as given. Never fails: an unparsable line simply yields no argument.
    pub fn from_canonical(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The canonical text.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.0
    }

    /// The value of `name`, or `None` when the name is absent or its line is malformed.
    ///
    /// Lines are read in order. Reading stops at the first malformed line, so nothing after
    /// it can be found: an unreadable argument is absent, never guessed. The first of two
    /// lines with the same name wins.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        let text = self.0.as_str();
        let mut pos = 0;
        while pos < text.len() {
            let (line_name, value, next) = read_line(text, pos)?;
            if line_name == name {
                return Some(value);
            }
            pos = next;
        }
        None
    }
}

/// Reads one `name:len:value\n` line starting at `pos`; `None` when it is malformed.
fn read_line(text: &str, pos: usize) -> Option<(&str, &str, usize)> {
    let rest = text.get(pos..)?;
    let (name, after_name) = rest.split_once(':')?;
    if name.is_empty() || name.contains('\n') {
        return None;
    }
    let (len_text, after_len) = after_name.split_once(':')?;
    if len_text.is_empty() || !len_text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let len: usize = len_text.parse().ok()?;
    let value_start = pos + name.len() + 1 + len_text.len() + 1;
    let value_end = value_start.checked_add(len)?;
    let value = text.get(value_start..value_end)?;
    debug_assert!(after_len.starts_with(value));
    match text.get(value_end..)? {
        "" => Some((name, value, value_end)),
        tail if tail.starts_with('\n') => Some((name, value, value_end + 1)),
        _ => None,
    }
}

/// One management path that the verified safety set does not cover.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ManagementPath(String);

impl ManagementPath {
    /// Validates `input`: 1 to 310 printable ASCII characters, no spaces.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        if input.is_empty() || input.len() > 310 {
            return Err(ValidationError::new(
                "management path",
                "must be 1 to 310 characters",
            ));
        }
        if !input.chars().all(|c| c.is_ascii_graphic()) {
            return Err(ValidationError::new(
                "management path",
                "must be printable ASCII without spaces",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The path as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ManagementPath {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ManagementPath> for String {
    fn from(value: ManagementPath) -> Self {
        value.0
    }
}

/// The containment posture, computed elsewhere and supplied as data.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ContainmentInput {
    /// A verified, fresh helper attestation exists.
    pub attested: bool,
    /// Discovered management paths absent from the verified effective set.
    pub uncovered_paths: Vec<ManagementPath>,
    /// A previously owner-confirmed path is older than its confirmation bound.
    pub stale_confirmation: bool,
    /// The containment target overlaps the verified effective set, which must be refused.
    pub target_overlaps_verified_set: bool,
    /// The helper reported that it cannot compute its safety set at all.
    pub helper_incomplete: bool,
}

impl ContainmentInput {
    /// The value for a request whose contract is not `Contain`.
    #[must_use]
    pub fn not_applicable() -> Self {
        Self::default()
    }

    /// Whether this value is the not-applicable value.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Autonomy and circuit-breaker state, supplied as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AutonomyState {
    /// The global or host autonomy pause is in effect.
    pub paused: bool,
    /// The matched rules are circuit-broken after repeated failures.
    pub circuit_broken: bool,
}

/// Everything the evaluator needs. Built by the caller from ports and repositories; nothing
/// here is fetched, and nothing is looked up during evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluateRequest {
    /// The host the request is for.
    pub host: HostId,
    /// The resolved contract. An unknown contract cannot be expressed.
    pub contract: ContractMeta,
    /// The contract version, carried so the trace names what was evaluated.
    pub contract_version: ContractVersion,
    /// The owner-typed name the request names, for rule matching.
    pub target: TargetName,
    /// Its canonical identity, for pinning and permit binding.
    pub target_identity: TargetIdentity,
    /// The canonical arguments.
    pub args: CanonicalArgs,
    /// The rate the request asks for; `None` when the capability has no rate notion.
    pub requested_rate_per_hour: Option<u32>,
    /// The TTL the request asks for; `None` when the capability has no TTL notion.
    pub requested_ttl: Option<SignedDuration>,
    /// The pinned subjects for this host and operation.
    pub pinned: BTreeSet<TargetIdentity>,
    /// Resolutions of the target names the matching rules may name. A name that failed to
    /// resolve is absent, which contributes nothing.
    pub rule_targets: BTreeMap<TargetName, TargetIdentity>,
    /// The host's last reported ceiling summary.
    pub ceiling: Option<CeilingSummary>,
    /// The ceiling hash the caller expects, on history replay.
    pub expected_ceiling_hash: Option<String>,
    /// Every safety fact the hub knows for this host. Absent facts are treated as unknown.
    pub facts: BTreeMap<FactId, FactState>,
    /// Whether the host's agent runs as root.
    pub runs_as_root: bool,
    /// The containment posture.
    pub containment: ContainmentInput,
    /// The autonomy state.
    pub autonomy: AutonomyState,
    /// The evaluation instant, the only time input.
    pub now: Timestamp,
    /// Opaque tag of the policy version being evaluated.
    pub policy_version: Option<Uuid>,
}

/// The `ContractMeta` of `id`, checked against the index.
///
/// Callers build [`EvaluateRequest`] with the `Ok` value, so an unknown contract can never
/// reach evaluation and can never default to anything.
pub fn resolve_contract(
    index: &ContractIndex,
    id: &ContractId,
) -> Result<ContractMeta, PolicyError> {
    let meta = index
        .meta(id)
        .ok_or_else(|| PolicyError::UnknownContract(id.clone()))?;
    if index.effect_of_category(&meta.category).is_none() {
        return Err(PolicyError::CategoryNotInIndex(meta.category));
    }
    Ok(meta)
}

/// The union of every `Exact` target set over `rules`, so the caller can resolve exactly the
/// names evaluation may need. This is the only coupling between the evaluator and the target
/// resolver, and it runs one way.
#[must_use]
pub fn names_to_resolve(rules: &[Rule]) -> BTreeSet<TargetName> {
    rules
        .iter()
        .filter_map(|rule| match &rule.targets {
            TargetScope::Exact(names) => Some(names),
            TargetScope::Any => None,
        })
        .flatten()
        .cloned()
        .collect()
}
