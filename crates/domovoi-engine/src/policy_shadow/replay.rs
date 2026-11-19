//! Re-runs a recorded evaluation: rebuilds the exact request from the stored inputs and
//! compares the result with the decision recorded for it.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::capability::{ContractVersion, FactId, TargetIdentity, TargetName};
use domovoi_core::policy_store::{ShadowRecord, ShadowStatus};
use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use uuid::Uuid;

use super::doc::{DecisionEnvelope, VERSION};
use super::recorded::{InputsEnvelope, RecordedInputs};
use crate::policy::{EvaluateRequest, Rule, RuleSelection, simulate};

/// Why a stored document could not be replayed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    /// The document names a version this build does not know.
    #[error("unsupported document version {0}")]
    Version(u64),
    /// The document is not valid JSON of the expected shape.
    #[error("stored document is not valid: {0}")]
    Json(String),
    /// A stored value no longer parses.
    #[error("stored value for {0} no longer parses")]
    Field(&'static str),
}

/// Reads a stored `RecordedInputs` envelope.
pub fn decode_inputs(text: &str) -> Result<RecordedInputs, ReplayError> {
    let probe: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ReplayError::Json(e.to_string()))?;
    let v = probe
        .get("v")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ReplayError::Json("missing or non-integer version".to_owned()))?;
    if v != VERSION {
        return Err(ReplayError::Version(v));
    }
    let envelope: InputsEnvelope =
        serde_json::from_value(probe).map_err(|e| ReplayError::Json(e.to_string()))?;
    Ok(envelope.inputs)
}

/// Reads a stored decision envelope.
pub fn decode_decision(text: &str) -> Result<DecisionEnvelope, ReplayError> {
    DecisionEnvelope::from_json(text).ok_or_else(|| ReplayError::Json("decision".to_owned()))
}

/// Rebuilds the exact request `evaluate` saw. The expected ceiling hash is the recorded
/// summary's own hash, which changes no result: a request is only refused when the two
/// differ.
pub fn request_from_inputs(
    inputs: &RecordedInputs,
    version: Option<Uuid>,
) -> Result<EvaluateRequest, ReplayError> {
    let field = ReplayError::Field;
    let contract = inputs.contract.to_meta().ok_or(field("contract"))?;
    let pinned: Result<BTreeSet<TargetIdentity>, _> = inputs
        .pinned
        .iter()
        .map(|text| TargetIdentity::parse(text))
        .collect();
    let mut rule_targets = BTreeMap::new();
    for (name, identity) in &inputs.rule_targets {
        rule_targets.insert(
            TargetName::parse(name).map_err(|_| field("rule_targets"))?,
            TargetIdentity::parse(identity).map_err(|_| field("rule_targets"))?,
        );
    }
    let mut facts = BTreeMap::new();
    for (fact, state) in &inputs.facts {
        facts.insert(FactId::parse(fact).map_err(|_| field("facts"))?, *state);
    }
    Ok(EvaluateRequest {
        host: inputs.host,
        contract,
        contract_version: ContractVersion::new(inputs.contract_version)
            .map_err(|_| field("contract_version"))?,
        target: TargetName::parse(&inputs.target).map_err(|_| field("target"))?,
        target_identity: TargetIdentity::parse(&inputs.target_identity)
            .map_err(|_| field("target_identity"))?,
        args: crate::policy::CanonicalArgs::from_canonical(inputs.args.clone()),
        requested_rate_per_hour: inputs.requested_rate_per_hour,
        requested_ttl: inputs.requested_ttl_secs.map(SignedDuration::from_secs),
        pinned: pinned.map_err(|_| field("pinned"))?,
        rule_targets,
        ceiling: inputs.ceiling.clone(),
        expected_ceiling_hash: inputs.ceiling.as_ref().and_then(|c| c.hash.clone()),
        facts,
        runs_as_root: inputs.runs_as_root,
        containment: inputs.containment.clone(),
        autonomy: inputs.autonomy,
        now: inputs.now.parse::<Timestamp>().map_err(|_| field("now"))?,
        policy_version: version,
    })
}

/// How a re-run compares with the recorded decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    /// The same decision.
    Same,
    /// A different decision.
    Changed,
    /// The row is not evaluated, or one of its columns no longer decodes.
    NotReplayable,
}

/// One re-run next to what the row recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct Replayed {
    /// What the rules decide now.
    pub simulated: DecisionEnvelope,
    /// What the row recorded for the same selection, when it could be read.
    pub baseline: Option<DecisionEnvelope>,
    /// How the two compare.
    pub comparison: Comparison,
}

/// The recorded decision column for `selection`.
fn recorded_column(record: &ShadowRecord, selection: RuleSelection) -> Option<&str> {
    match selection {
        RuleSelection::Enforced => record.enforced.as_deref(),
        RuleSelection::Shadow => record.shadow.as_deref(),
    }
}

/// Compares `simulated` with what `record` stored for `selection`.
#[must_use]
pub fn compare_with_record(
    record: &ShadowRecord,
    selection: RuleSelection,
    simulated: DecisionEnvelope,
) -> Replayed {
    let baseline = (record.status == ShadowStatus::Evaluated)
        .then(|| recorded_column(record, selection))
        .flatten()
        .and_then(DecisionEnvelope::from_json);
    let comparison = match &baseline {
        None => Comparison::NotReplayable,
        Some(base) if *base == simulated => Comparison::Same,
        Some(_) => Comparison::Changed,
    };
    Replayed {
        simulated,
        baseline,
        comparison,
    }
}

/// A result for a row that cannot be replayed.
fn not_replayable() -> Replayed {
    Replayed {
        simulated: DecisionEnvelope::Error {
            code: "not_replayable".to_owned(),
        },
        baseline: None,
        comparison: Comparison::NotReplayable,
    }
}

/// Evaluates `rules` for one recorded row under `selection` and compares it with the row's
/// own decision for that selection. Deterministic: the same rules and row always give an equal
/// result. Replaying a row against the rules it was recorded under gives `Same`.
#[must_use]
pub fn replay(rules: &[Rule], record: &ShadowRecord, selection: RuleSelection) -> Replayed {
    if record.status != ShadowStatus::Evaluated {
        return not_replayable();
    }
    let Some(text) = record.inputs.as_deref() else {
        return not_replayable();
    };
    let Ok(inputs) = decode_inputs(text) else {
        return not_replayable();
    };
    let Ok(request) = request_from_inputs(&inputs, None) else {
        return not_replayable();
    };
    let result = simulate(rules, std::slice::from_ref(&request), selection)
        .into_iter()
        .next();
    let Some(result) = result else {
        return not_replayable();
    };
    compare_with_record(record, selection, DecisionEnvelope::from_result(&result))
}
