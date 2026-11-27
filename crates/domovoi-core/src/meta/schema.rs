//! JSON Schemas of the meta-tools, as shown to the model.

use serde_json::{Value, json};

use super::MetaTool;
use crate::capability::EffectClass;
use crate::triage::{
    HEADLINE_MAX_CHARS, MANUAL_STEP_MAX_CHARS, MAX_EVIDENCE, MAX_MANUAL_STEPS, MAX_PROPOSALS,
    NEXT_STEP_MAX_CHARS, ReadTool, argument_summary,
};

/// Longest capability id, in characters.
pub(super) const CONTRACT_MAX_CHARS: usize = 80;
/// Longest host or target name in a plan step.
pub(super) const NAME_MAX_CHARS: usize = 128;
/// Longest plan step reason.
pub(super) const REASON_MAX_CHARS: usize = 200;
/// Longest `retry_of.changed`.
pub(super) const CHANGED_MAX_CHARS: usize = 280;
/// Longest experience search query.
pub(super) const QUERY_MAX_CHARS: usize = 200;
/// Longest question to the owner.
pub(super) const QUESTION_MAX_CHARS: usize = 500;
/// Most arguments in a plan step.
pub(super) const STEP_ARGS_MAX: usize = 32;
/// Longest argument key.
pub(super) const ARG_KEY_MAX_CHARS: usize = 64;
/// Longest argument value.
pub(super) const ARG_VALUE_MAX_CHARS: usize = 1024;

fn closed(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

impl MetaTool {
    /// The JSON Schema of this tool's input object.
    #[must_use]
    pub fn json_schema(&self) -> Value {
        match self {
            Self::ListCapabilities => closed(
                json!({
                    "effect": { "type": "string", "enum": EffectClass::ALL.map(|e| e.as_str()) },
                    "include_unavailable": { "type": "boolean", "default": false },
                }),
                &[],
            ),
            Self::DescribeCapability => closed(
                json!({ "contract": {
                    "type": "string",
                    "maxLength": CONTRACT_MAX_CHARS,
                    "pattern": r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$",
                } }),
                &["contract"],
            ),
            Self::RequestEvidence => closed(
                json!({
                    "tool": { "type": "string", "enum": ReadTool::ALL.map(|t| t.name()) },
                    "args": {
                        "type": "object",
                        "default": {},
                        "description": format!(
                            "The arguments of the chosen tool (* is required): {}",
                            argument_summary()
                        ),
                    },
                }),
                &["tool"],
            ),
            Self::SearchExperience => closed(
                json!({ "query": { "type": "string", "minLength": 1, "maxLength": QUERY_MAX_CHARS } }),
                &["query"],
            ),
            Self::ProposePlan => closed(
                json!({ "steps": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_PROPOSALS,
                    "items": step_schema(),
                } }),
                &["steps"],
            ),
            Self::AskUser => closed(
                json!({ "question": { "type": "string", "minLength": 1, "maxLength": QUESTION_MAX_CHARS } }),
                &["question"],
            ),
            Self::Finish => closed(
                finish_properties(),
                &["headline", "summary", "cause", "verdict", "next_step"],
            ),
        }
    }
}

fn step_schema() -> Value {
    closed(
        json!({
            "contract": { "type": "string", "maxLength": CONTRACT_MAX_CHARS },
            "args": {
                "type": "object",
                "maxProperties": STEP_ARGS_MAX,
                "additionalProperties": { "type": ["string", "integer", "boolean"], "maxLength": ARG_VALUE_MAX_CHARS },
            },
            "host": { "type": "string", "maxLength": NAME_MAX_CHARS },
            "target": { "type": "string", "maxLength": NAME_MAX_CHARS },
            "reason": { "type": "string", "minLength": 1, "maxLength": REASON_MAX_CHARS },
            "retry_of": closed(
                json!({
                    "step": { "type": "string" },
                    "changed": { "type": "string", "minLength": 1, "maxLength": CHANGED_MAX_CHARS },
                }),
                &["step", "changed"],
            ),
        }),
        &["contract", "reason"],
    )
}

fn finish_properties() -> Value {
    json!({
        "headline": { "type": "string", "minLength": 1, "maxLength": HEADLINE_MAX_CHARS },
        "summary": { "type": "string" },
        "cause": { "type": "string" },
        "evidence": {
            "type": "array",
            "maxItems": MAX_EVIDENCE,
            "items": closed(
                json!({ "label": { "type": "string" }, "value": { "type": "string" }, "source": { "type": "string" } }),
                &["label", "value", "source"],
            ),
        },
        "verdict": { "type": "string", "enum": ["ACTION_NEEDED", "MONITOR", "EXPECTED", "FIXED"] },
        "next_step": { "type": "string", "maxLength": NEXT_STEP_MAX_CHARS },
        "manual_steps": {
            "type": "array",
            "maxItems": MAX_MANUAL_STEPS,
            "items": { "type": "string", "maxLength": MANUAL_STEP_MAX_CHARS },
        },
        "details": { "type": "string" },
    })
}
