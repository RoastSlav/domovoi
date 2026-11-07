//! The bounded conversation with one provider: turns until the model stops calling tools
//! or the tool-call or time budget runs out.

use std::sync::Arc;

use domovoi_core::ports::{
    LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role, ToolSpec,
};
use domovoi_core::{Incident, ReadTool, SkillSummary};
use jiff::SignedDuration;

use super::{Shared, tools};

/// Sent once the budget is spent, with the last tool results.
pub const BUDGET_EXHAUSTED: &str = "Budget exhausted. Reply now with the JSON report.";

/// What a tool call gets once no calls are left.
pub const NO_CALLS_LEFT: &str = "error: the tool-call budget is spent; answer now";

/// One tool call the model made.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolUse {
    /// The tool name, validated against [`ReadTool::ALL`], or `unknown tool`.
    pub tool: String,
    /// For `run_read_skill`, the skill, only when the host catalogues it.
    pub skill: Option<String>,
    /// Whether the call was run and failed.
    pub failed: bool,
    /// The call was not run because the tool-call budget was spent.
    pub refused: bool,
    /// The model-supplied target, cut short, that matched neither.
    pub raw: Option<String>,
}

/// The tools offered to a provider that supports them: every read tool and nothing else.
#[must_use]
pub fn read_tools() -> Vec<ToolSpec> {
    ReadTool::ALL.into_iter().map(ToolSpec::from).collect()
}

/// The skill for a `run_read_skill` call's input, `None` for the other tools. Read from
/// the raw input so a call that fails to parse still names the skill it targeted.
fn read_skill_of(name: &str, input: &serde_json::Value) -> Option<String> {
    if name != "run_read_skill" {
        return None;
    }
    input.get("skill")?.as_str().map(str::to_owned)
}

/// Longest model-supplied target stored in a timeline entry: the names are untrusted
/// text, so only a short piece of an unknown one is worth keeping.
const MAX_RAW_TOOL_USE: usize = 96;

/// Recorded shape `(tool, skill, raw)` of a model call. A name outside the closed
/// [`ReadTool::ALL`] collapses into one `unknown tool` row; a skill counts only when
/// the host catalogues it. The rejected target is kept cut short for the audit.
fn target_of(
    name: &str,
    skill: Option<String>,
    skills: &[SkillSummary],
) -> (String, Option<String>, Option<String>) {
    if !ReadTool::ALL.iter().any(|tool| tool.name() == name) {
        return (
            "unknown tool".to_owned(),
            None,
            Some(name.chars().take(MAX_RAW_TOOL_USE).collect()),
        );
    }
    match skill {
        None => (name.to_owned(), None, None),
        Some(candidate) if skills.iter().any(|s| s.name.as_str() == candidate) => {
            (name.to_owned(), Some(candidate), None)
        }
        Some(candidate) => (
            name.to_owned(),
            None,
            Some(candidate.chars().take(MAX_RAW_TOOL_USE).collect()),
        ),
    }
}

/// The text parts of `parts`, joined.
pub(crate) fn text_of(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runs one model turn and records its usage.
pub(crate) async fn complete(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    req: LlmRequest,
) -> Result<LlmResponse, LlmError> {
    let response = provider.complete(req).await?;
    let usage = response.usage;
    crate::telemetry::record_llm(
        provider.id(),
        usage.input_tokens,
        usage.output_tokens,
        usage.cost_micros,
    );
    if let Err(err) = shared
        .deps
        .usage
        .record(
            provider.id(),
            u64::from(usage.input_tokens),
            u64::from(usage.output_tokens),
            usage.cost_micros.unwrap_or(0),
            shared.deps.clock.now(),
        )
        .await
    {
        tracing::warn!(%err, "could not record model usage");
    }
    Ok(response)
}

/// Continues `messages` with `provider` until the model answers without tool calls, and
/// returns that answer's text. Tool calls are answered through [`tools::call`] within the
/// run budget; once it is spent the model is told to answer and further calls are not
/// run. Every call the model makes while it may still call tools is pushed onto `used`;
/// calls in the response after it was told to answer are ignored.
pub(crate) async fn converse(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    incident: &Incident,
    skills: &[SkillSummary],
    system: &str,
    messages: &mut Vec<LlmMessage>,
    used: &mut Vec<ToolUse>,
) -> Result<String, LlmError> {
    let budget = shared.settings.budget;
    let specs = if provider.supports_tools() {
        read_tools()
    } else {
        Vec::new()
    };
    let start = shared.deps.clock.now();
    let deadline = start
        .checked_add(SignedDuration::from_secs(i64::from(budget.max_seconds)))
        .unwrap_or(start);
    let mut calls_used: u16 = 0;
    let mut told_to_answer = false;
    loop {
        let req = LlmRequest {
            system: system.to_owned(),
            messages: messages.clone(),
            tools: specs.clone(),
            max_output_tokens: budget.max_output_tokens,
        };
        let response = complete(shared, provider, req).await?;
        messages.push(LlmMessage {
            role: Role::Assistant,
            parts: response.parts.clone(),
        });
        let calls: Vec<(String, String, serde_json::Value)> = response
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::ToolCall { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect();
        if calls.is_empty() || told_to_answer || specs.is_empty() {
            return Ok(text_of(&response.parts));
        }
        let mut results = Vec::with_capacity(calls.len() + 1);
        for (id, name, input) in calls {
            let skill = read_skill_of(&name, &input);
            let refused = calls_used >= budget.max_tool_calls;
            let result = if refused {
                tools::ToolResult {
                    content: NO_CALLS_LEFT.to_owned(),
                    is_error: true,
                }
            } else {
                calls_used += 1;
                tools::call(shared, incident, skills, &name, input).await
            };
            let (tool, used_skill, raw) = target_of(&name, skill, skills);
            used.push(ToolUse {
                tool,
                skill: used_skill,
                failed: !refused && result.is_error,
                refused,
                raw,
            });
            results.push(Part::ToolResult {
                id,
                content: result.content,
                is_error: result.is_error,
            });
        }
        if calls_used >= budget.max_tool_calls || shared.deps.clock.now() >= deadline {
            results.push(Part::Text(BUDGET_EXHAUSTED.to_owned()));
            told_to_answer = true;
        }
        messages.push(LlmMessage {
            role: Role::User,
            parts: results,
        });
    }
}
