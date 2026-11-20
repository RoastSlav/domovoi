//! Parsing a model's meta-tool call into a typed, bounded value.

use serde::Deserialize;
use serde_json::Value;

use super::plan_input::PlanInput;
use super::schema::{
    ARG_KEY_MAX_CHARS, ARG_VALUE_MAX_CHARS, CHANGED_MAX_CHARS, CONTRACT_MAX_CHARS, NAME_MAX_CHARS,
    QUERY_MAX_CHARS, QUESTION_MAX_CHARS, REASON_MAX_CHARS, STEP_ARGS_MAX,
};
use super::{MetaTool, PlanStepInput};
use crate::capability::EffectClass;
use crate::triage::{MAX_PROPOSALS, ReadTool, ReadToolCall, ToolArgError, TriageReport};

/// One parsed call to a [`MetaTool`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaToolCall {
    /// List capabilities.
    ListCapabilities {
        /// Only this effect class.
        effect: Option<EffectClass>,
        /// Also list what is not available now.
        include_unavailable: bool,
    },
    /// Describe one capability; the engine parses the id.
    DescribeCapability {
        /// The raw capability id, bounded and free of control characters.
        contract: String,
    },
    /// Read evidence.
    RequestEvidence(ReadToolCall),
    /// Search past operations on the loop's own host.
    SearchExperience {
        /// The trimmed query text: 1 to 200 characters, with at least one letter or digit.
        query: String,
    },
    /// Suggest steps.
    ProposePlan(PlanInput),
    /// Ask the owner a question.
    AskUser {
        /// The question.
        question: String,
    },
    /// End with a report; its `proposals` are always empty.
    Finish(Box<TriageReport>),
}

impl MetaToolCall {
    /// The tool this call is for.
    #[must_use]
    pub fn tool(&self) -> MetaTool {
        match self {
            Self::ListCapabilities { .. } => MetaTool::ListCapabilities,
            Self::DescribeCapability { .. } => MetaTool::DescribeCapability,
            Self::RequestEvidence(_) => MetaTool::RequestEvidence,
            Self::SearchExperience { .. } => MetaTool::SearchExperience,
            Self::ProposePlan(_) => MetaTool::ProposePlan,
            Self::AskUser { .. } => MetaTool::AskUser,
            Self::Finish(_) => MetaTool::Finish,
        }
    }

    /// Parses and bounds the raw input of a call to `name`.
    pub fn from_json(name: &str, input: Value) -> Result<Self, MetaArgError> {
        let tool =
            MetaTool::from_name(name).ok_or_else(|| MetaArgError::UnknownTool(name.to_owned()))?;
        let bad = |reason: String| MetaArgError::InvalidArgs {
            tool: tool.name(),
            reason,
        };
        match tool {
            MetaTool::ListCapabilities => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    #[serde(default)]
                    effect: Option<EffectClass>,
                    #[serde(default)]
                    include_unavailable: bool,
                }
                let args: Args = parse(input).map_err(bad)?;
                Ok(Self::ListCapabilities {
                    effect: args.effect,
                    include_unavailable: args.include_unavailable,
                })
            }
            MetaTool::DescribeCapability => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    contract: String,
                }
                let args: Args = parse(input).map_err(bad)?;
                text("contract", &args.contract, 1, CONTRACT_MAX_CHARS).map_err(bad)?;
                Ok(Self::DescribeCapability {
                    contract: args.contract,
                })
            }
            MetaTool::RequestEvidence => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    tool: String,
                    #[serde(default)]
                    args: Value,
                }
                let args: Args = parse(input).map_err(bad)?;
                let inner = match args.args {
                    Value::Null => Value::Object(serde_json::Map::new()),
                    other => other,
                };
                ReadToolCall::from_json(&args.tool, inner)
                    .map(Self::RequestEvidence)
                    .map_err(|error| match error {
                        ToolArgError::UnknownTool(unknown) => MetaArgError::InvalidArgs {
                            tool: tool.name(),
                            reason: format!("unknown tool `{unknown}`"),
                        },
                        ToolArgError::InvalidArgs { reason, .. } => bad(reason),
                    })
            }
            MetaTool::SearchExperience => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    query: String,
                }
                let args: Args = parse(input).map_err(bad)?;
                let query = args.query.trim();
                let length = query.chars().count();
                if length == 0 || length > QUERY_MAX_CHARS || query.chars().any(char::is_control) {
                    return Err(bad("query: write 1 to 200 characters".to_owned()));
                }
                if !query.chars().any(char::is_alphanumeric) {
                    return Err(bad("query: needs at least one word".to_owned()));
                }
                Ok(Self::SearchExperience {
                    query: query.to_owned(),
                })
            }
            MetaTool::ProposePlan => {
                let plan: PlanInput = parse(input).map_err(bad)?;
                check_plan(&plan).map_err(bad)?;
                Ok(Self::ProposePlan(plan))
            }
            MetaTool::AskUser => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Args {
                    question: String,
                }
                let args: Args = parse(input).map_err(bad)?;
                text("question", &args.question, 1, QUESTION_MAX_CHARS).map_err(bad)?;
                Ok(Self::AskUser {
                    question: args.question,
                })
            }
            MetaTool::Finish => {
                if input.get("proposals").is_some() {
                    return Err(bad(
                        "proposals is not part of finish: use propose_plan".to_owned()
                    ));
                }
                let report: TriageReport = parse(input).map_err(bad)?;
                report
                    .validate()
                    .map_err(|error| bad(format!("{}: {}", error.field, error.reason)))?;
                Ok(Self::Finish(Box::new(report)))
            }
        }
    }

    /// Whether `name` is a read-tool name the model used in place of `request_evidence`.
    #[must_use]
    pub fn is_old_read_name(name: &str) -> bool {
        ReadTool::from_name(name).is_some()
    }
}

/// Why a meta-tool call was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetaArgError {
    /// The name is not a meta-tool.
    #[error("unknown tool `{0}`")]
    UnknownTool(String),
    /// The arguments are malformed or out of range.
    #[error("{tool}: {reason}")]
    InvalidArgs {
        /// The tool's name.
        tool: &'static str,
        /// What is wrong.
        reason: String,
    },
}

fn parse<T: serde::de::DeserializeOwned>(input: Value) -> Result<T, String> {
    // A model that sends no arguments at all means an empty object.
    let input = match input {
        Value::Null => Value::Object(serde_json::Map::new()),
        other => other,
    };
    serde_json::from_value(input).map_err(|error| error.to_string())
}

fn text(field: &str, value: &str, min: usize, max: usize) -> Result<(), String> {
    let count = value.chars().count();
    if count < min || count > max {
        return Err(format!("{field}: must be {min} to {max} characters"));
    }
    if value.trim().is_empty() {
        return Err(format!("{field}: must not be blank"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field}: must not contain control characters"));
    }
    Ok(())
}

fn check_plan(plan: &PlanInput) -> Result<(), String> {
    if plan.steps.is_empty() || plan.steps.len() > MAX_PROPOSALS {
        return Err(format!("steps: must have 1 to {MAX_PROPOSALS} items"));
    }
    plan.steps.iter().try_for_each(check_step)
}

fn check_step(step: &PlanStepInput) -> Result<(), String> {
    text("contract", &step.contract, 1, CONTRACT_MAX_CHARS)?;
    text("reason", &step.reason, 1, REASON_MAX_CHARS)?;
    for (field, value) in [("host", &step.host), ("target", &step.target)] {
        if let Some(value) = value {
            text(field, value, 1, NAME_MAX_CHARS)?;
        }
    }
    if step.args.len() > STEP_ARGS_MAX {
        return Err(format!("args: must have at most {STEP_ARGS_MAX} items"));
    }
    for (key, value) in &step.args {
        text("args key", key, 1, ARG_KEY_MAX_CHARS)?;
        let canonical = value.canonical();
        if canonical.chars().count() > ARG_VALUE_MAX_CHARS {
            return Err(format!(
                "args: value of `{key}` must be at most {ARG_VALUE_MAX_CHARS} characters"
            ));
        }
    }
    if let Some(retry) = &step.retry_of {
        text("retry_of.step", &retry.step, 1, 64)?;
        text("retry_of.changed", &retry.changed, 1, CHANGED_MAX_CHARS)?;
    }
    Ok(())
}
