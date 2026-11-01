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

/// The tools offered to a provider that supports them: every read tool and nothing else.
#[must_use]
pub fn read_tools() -> Vec<ToolSpec> {
    ReadTool::ALL.into_iter().map(ToolSpec::from).collect()
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
/// run. Providers without tool support get exactly one turn.
pub(crate) async fn converse(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    incident: &Incident,
    skills: &[SkillSummary],
    system: &str,
    messages: &mut Vec<LlmMessage>,
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
    let mut used: u16 = 0;
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
            let result = if used >= budget.max_tool_calls {
                tools::ToolResult {
                    content: NO_CALLS_LEFT.to_owned(),
                    is_error: true,
                }
            } else {
                used += 1;
                tools::call(shared, incident, skills, &name, input).await
            };
            results.push(Part::ToolResult {
                id,
                content: result.content,
                is_error: result.is_error,
            });
        }
        if used >= budget.max_tool_calls || shared.deps.clock.now() >= deadline {
            results.push(Part::Text(BUDGET_EXHAUSTED.to_owned()));
            told_to_answer = true;
        }
        messages.push(LlmMessage {
            role: Role::User,
            parts: results,
        });
    }
}
