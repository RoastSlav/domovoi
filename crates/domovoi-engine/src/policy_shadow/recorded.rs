//! The documents a shadow row stores: the exact evaluator inputs, the candidate step and the
//! list of inputs that were unavailable. None holds a secret.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::HostId;
use domovoi_core::capability::{
    CeilingSummary, ConstrainableArg, ConstraintKind, ContractCategory, ContractId, ContractMeta,
    EffectClass, FactId, TargetKind,
};
use domovoi_core::policy_store::ShadowCandidate;
use serde::{Deserialize, Serialize};

use super::doc::VERSION;
use crate::policy::{AutonomyState, ContainmentInput, FactState};

/// Which evaluator input a gap is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapInput {
    /// The contract.
    Contract,
    /// The target.
    Target,
    /// The unattended ceiling.
    Ceiling,
    /// Safety facts.
    Facts,
    /// Whether the agent runs as root.
    RunsAsRoot,
    /// The containment posture.
    Containment,
    /// The autonomy state.
    Autonomy,
    /// The pinned targets.
    PinnedTargets,
}

/// Why an input was unavailable. Each reason names who can fix it, so the owner can tell
/// "not built yet" from "not connected right now".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapReason {
    /// The feature that supplies it ships later: a ceiling the agent does not report yet,
    /// fact producers, the helper, the autonomy switches.
    NotProducedYet,
    /// No live session to ask.
    HostNotConnected,
    /// The hub tried and could not establish it (identity unproven, unknown contract).
    Unresolvable,
}

/// One input the recorder had to substitute, with the reason. A substituted input is always
/// the fail-closed value, never a made-up safe one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InputGap {
    /// The input.
    pub input: GapInput,
    /// Why it was unavailable.
    pub reason: GapReason,
}

/// One constrainable argument of the contract evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstrainableArgDoc {
    /// The argument key.
    pub key: String,
    /// `int_at_least` or `int_at_most`.
    pub kind: String,
    /// What the bound means.
    pub description: String,
}

/// The contract as the evaluator saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractDoc {
    /// The contract id.
    pub id: String,
    /// Its category.
    pub category: String,
    /// Its effect class.
    pub effect: EffectClass,
    /// What it acts on.
    pub target_kind: TargetKind,
    /// The facts it declares.
    pub safety_facts: Vec<String>,
    /// The arguments a ceiling may constrain.
    pub constrainable_args: Vec<ConstrainableArgDoc>,
}

impl ContractDoc {
    /// The document for `meta`.
    #[must_use]
    pub fn from_meta(meta: &ContractMeta) -> Self {
        Self {
            id: meta.id.to_string(),
            category: meta.category.to_string(),
            effect: meta.effect,
            target_kind: meta.target_kind,
            safety_facts: meta.safety_facts.iter().map(ToString::to_string).collect(),
            constrainable_args: meta
                .constrainable_args
                .iter()
                .map(|arg| ConstrainableArgDoc {
                    key: arg.key.clone(),
                    kind: match arg.kind {
                        ConstraintKind::IntAtLeast => "int_at_least",
                        ConstraintKind::IntAtMost => "int_at_most",
                    }
                    .to_owned(),
                    description: arg.description.clone(),
                })
                .collect(),
        }
    }

    /// The contract back, or `None` when a stored value no longer parses.
    #[must_use]
    pub fn to_meta(&self) -> Option<ContractMeta> {
        let mut args = Vec::new();
        for arg in &self.constrainable_args {
            let kind = match arg.kind.as_str() {
                "int_at_least" => ConstraintKind::IntAtLeast,
                "int_at_most" => ConstraintKind::IntAtMost,
                _ => return None,
            };
            args.push(ConstrainableArg {
                key: arg.key.clone(),
                kind,
                description: arg.description.clone(),
            });
        }
        let facts: Result<Vec<FactId>, _> =
            self.safety_facts.iter().map(|f| FactId::parse(f)).collect();
        Some(ContractMeta {
            id: ContractId::parse(&self.id).ok()?,
            category: ContractCategory::parse(&self.category).ok()?,
            effect: self.effect,
            target_kind: self.target_kind,
            safety_facts: facts.ok()?,
            constrainable_args: args,
        })
    }
}

/// Everything `evaluate` was given, in a form that replays exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedInputs {
    /// The host.
    pub host: HostId,
    /// The contract.
    pub contract: ContractDoc,
    /// The contract version.
    pub contract_version: u32,
    /// The target name text.
    pub target: String,
    /// The target identity text.
    pub target_identity: String,
    /// The canonical arguments, as given.
    pub args: String,
    /// The requested rate.
    pub requested_rate_per_hour: Option<u32>,
    /// The requested TTL in seconds.
    pub requested_ttl_secs: Option<i64>,
    /// The pinned target identities.
    pub pinned: BTreeSet<String>,
    /// Target name to identity for the names rules mention.
    pub rule_targets: BTreeMap<String, String>,
    /// The ceiling as reported.
    pub ceiling: Option<CeilingSummary>,
    /// Fact id to state.
    pub facts: BTreeMap<String, FactState>,
    /// Whether the agent runs as root (true when unknown).
    pub runs_as_root: bool,
    /// The containment posture.
    pub containment: ContainmentInput,
    /// The autonomy state.
    pub autonomy: AutonomyState,
    /// The evaluation instant, RFC 3339 UTC.
    pub now: String,
}

/// `{"v":1, ...}` around [`RecordedInputs`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputsEnvelope {
    /// The format version.
    pub v: u64,
    /// The inputs, flattened beside `v`.
    #[serde(flatten)]
    pub inputs: RecordedInputs,
}

impl InputsEnvelope {
    /// Wraps `inputs` at the current version.
    #[must_use]
    pub fn new(inputs: RecordedInputs) -> Self {
        Self { v: VERSION, inputs }
    }
}

/// One subject the operation pinned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedDoc {
    /// The host.
    pub host: String,
    /// The check.
    pub check: String,
    /// The target, if any.
    pub target: Option<String>,
}

/// The step that was evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateDoc {
    /// The execution id.
    pub execution: String,
    /// The operation id.
    pub operation: String,
    /// The host id.
    pub host: String,
    /// The contract id.
    pub contract: String,
    /// The contract version.
    pub contract_version: u32,
    /// The check.
    pub check: String,
    /// The target.
    pub target: Option<String>,
    /// The arguments.
    pub args: BTreeMap<String, String>,
    /// The subjects the operation pinned.
    pub pinned: Vec<PinnedDoc>,
    /// When the step was created, RFC 3339 UTC.
    pub created_at: String,
}

impl CandidateDoc {
    /// The document for `candidate`.
    #[must_use]
    pub fn from_candidate(candidate: &ShadowCandidate) -> Self {
        Self {
            execution: candidate.execution.to_string(),
            operation: candidate.operation.to_string(),
            host: candidate.host.to_string(),
            contract: candidate.contract.to_string(),
            contract_version: candidate.contract_version,
            check: candidate.check.to_string(),
            target: candidate.target.as_ref().map(ToString::to_string),
            args: candidate.args.clone(),
            pinned: candidate
                .pinned
                .iter()
                .map(|p| PinnedDoc {
                    host: p.host.to_string(),
                    check: p.check.to_string(),
                    target: p.target.as_ref().map(ToString::to_string),
                })
                .collect(),
            created_at: candidate.created_at.to_string(),
        }
    }

    /// The step back, or `None` when a stored value no longer parses.
    #[must_use]
    pub fn to_candidate(&self) -> Option<ShadowCandidate> {
        use domovoi_core::{CheckKey, ExecutionId, OperationId, PinnedSubject, TargetKey};
        let mut pinned = Vec::new();
        for p in &self.pinned {
            pinned.push(PinnedSubject {
                host: p.host.parse().ok()?,
                check: CheckKey::parse(&p.check).ok()?,
                target: match &p.target {
                    Some(t) => Some(TargetKey::parse(t).ok()?),
                    None => None,
                },
            });
        }
        Some(ShadowCandidate {
            execution: self.execution.parse::<ExecutionId>().ok()?,
            operation: self.operation.parse::<OperationId>().ok()?,
            host: self.host.parse().ok()?,
            contract: ContractId::parse(&self.contract).ok()?,
            contract_version: self.contract_version,
            check: CheckKey::parse(&self.check).ok()?,
            target: match &self.target {
                Some(t) => Some(TargetKey::parse(t).ok()?),
                None => None,
            },
            args: self.args.clone(),
            pinned,
            created_at: self.created_at.parse().ok()?,
        })
    }
}
