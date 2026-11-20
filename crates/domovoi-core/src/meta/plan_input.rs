//! The typed input of `propose_plan`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::triage::ActionProposal;

/// A plan as the model wrote it: one to three steps, not yet checked against anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanInput {
    /// The steps, in the order the model gave them.
    pub steps: Vec<PlanStepInput>,
}

/// One suggested step as the model wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanStepInput {
    /// The capability id, for example `skill.restart_container`.
    pub contract: String,
    /// The arguments the model chose.
    #[serde(default)]
    pub args: BTreeMap<String, ArgValue>,
    /// A host name; the pinned host when omitted and unambiguous.
    #[serde(default)]
    pub host: Option<String>,
    /// A target name; the pinned target when omitted.
    #[serde(default)]
    pub target: Option<String>,
    /// Why the model suggests it.
    pub reason: String,
    /// Set when this step repeats one that failed earlier.
    #[serde(default)]
    pub retry_of: Option<RetryOfInput>,
}

/// A claim that a step repeats an earlier failed one, with what changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryOfInput {
    /// The earlier step's id.
    pub step: String,
    /// What is different this time.
    pub changed: String,
}

/// A step argument as the model wrote it: a string, an integer or a boolean. Floats, null,
/// arrays and objects do not deserialize. Skill arguments travel as strings, so
/// [`canonical`](Self::canonical) is what is checked and stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    /// A string.
    Text(String),
    /// An integer.
    Int(i64),
    /// A boolean.
    Flag(bool),
}

impl ArgValue {
    /// The string form checked against the manifest and stored.
    #[must_use]
    pub fn canonical(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Int(number) => number.to_string(),
            Self::Flag(flag) => flag.to_string(),
        }
    }
}

impl PlanInput {
    /// Turns the proposals of a text report into plan steps; `None` when there are none.
    #[must_use]
    pub fn from_proposals(proposals: &[ActionProposal]) -> Option<PlanInput> {
        if proposals.is_empty() {
            return None;
        }
        let steps = proposals
            .iter()
            .map(|proposal| PlanStepInput {
                contract: format!("skill.{}", proposal.skill),
                args: proposal
                    .args
                    .iter()
                    .map(|(key, value)| (key.clone(), ArgValue::Text(value.clone())))
                    .collect(),
                host: None,
                target: None,
                reason: proposal.reason.clone(),
                retry_of: None,
            })
            .collect();
        Some(PlanInput { steps })
    }

    /// Canonical string arguments of `step`.
    #[must_use]
    pub fn canonical_args(step: &PlanStepInput) -> BTreeMap<String, String> {
        step.args
            .iter()
            .map(|(key, value)| (key.clone(), value.canonical()))
            .collect()
    }
}
