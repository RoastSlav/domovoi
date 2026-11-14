//! The bounded conversation with one provider: turns until the model stops calling tools
//! or the tool-call or time budget runs out.

use std::sync::Arc;

use domovoi_core::ports::{
    LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role, StopReason, ToolSpec,
};
use domovoi_core::{ReadTool, SkillSummary};
use jiff::{SignedDuration, Timestamp};

use super::{Shared, tools};

/// Sent once the budget is spent, with the last tool results.
pub const BUDGET_EXHAUSTED: &str = "Budget exhausted. Reply now with the JSON report.";

/// What a tool call gets once no calls are left.
pub const NO_CALLS_LEFT: &str = "error: the tool-call budget is spent; answer now";

/// How much a turn's output allowance grows when a model runs out of room before it
/// finishes. A reasoning model counts its reasoning against the allowance, so the
/// configured size can be too small for it to say anything at all.
const GROWN_OUTPUT_ALLOWANCE: u32 = 4;

/// The most one turn may ask a provider for, whatever the run's total budget says. A model
/// that needs more than this to answer needs a larger `[triage] per_run.output_tokens`.
const MAX_OUTPUT_ALLOWANCE: u32 = 16_000;

/// The allowance a retried turn asks for, or `None` when it is already at
/// [`MAX_OUTPUT_ALLOWANCE`].
pub(crate) fn grown_allowance(current: u32) -> Option<u32> {
    let next = current
        .saturating_mul(GROWN_OUTPUT_ALLOWANCE)
        .min(MAX_OUTPUT_ALLOWANCE);
    (next > current).then_some(next)
}

/// The fixed context of one conversation: its system prompt, the tools' scope and the
/// skills offered.
pub(crate) struct Brief<'a> {
    pub(crate) system: &'a str,
    pub(crate) scope: tools::Scope,
    pub(crate) skills: &'a [SkillSummary],
}

/// What bounds one model run: when it must be finished, and how much output it may still
/// ask providers to generate across its turns and the retries they need.
///
/// Every request is charged here before it is sent, so neither a long tool loop nor a
/// growing allowance can run past the run's budget.
#[derive(Debug)]
pub(crate) struct RunBudget {
    per_turn: u32,
    remaining: u32,
    deadline: Timestamp,
}

impl RunBudget {
    /// The budget and deadline of a run served by `shared`. One triage run shares them
    /// across every provider it tries, so slow fallbacks do not extend the deadline.
    pub(crate) fn for_run(shared: &Shared) -> Self {
        let start = shared.deps.clock.now();
        let deadline = start
            .checked_add(SignedDuration::from_secs(i64::from(
                shared.settings.budget.max_seconds,
            )))
            .unwrap_or(start);
        Self {
            per_turn: shared.settings.budget.max_output_tokens,
            remaining: shared.settings.budget.max_total_output_tokens,
            deadline,
        }
    }

    /// When the run must be finished.
    pub(crate) fn deadline(&self) -> Timestamp {
        self.deadline
    }

    /// What a turn asks for before any retry.
    pub(crate) fn per_turn(&self) -> u32 {
        self.per_turn
    }

    /// Grants `wanted` tokens for one request, or what is left of the run's budget when that
    /// is smaller. `None` when nothing is left, so the request is not sent.
    pub(crate) fn grant(&mut self, wanted: u32) -> Option<u32> {
        let granted = wanted.min(self.remaining);
        (granted > 0).then(|| {
            self.remaining -= granted;
            granted
        })
    }

    /// [`grant`](Self::grant), with the failure a model run ends with when the budget is
    /// spent.
    pub(crate) fn grant_or_fail(&mut self, wanted: u32) -> Result<u32, LlmError> {
        self.grant(wanted).ok_or_else(|| {
            LlmError::Unavailable(
                "the run spent its whole output token budget before the model finished; \
                 raise [triage] per_run.total_tokens"
                    .to_owned(),
            )
        })
    }
}

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

/// Runs one model turn, bounded by `deadline`, and records its usage.
///
/// The deadline covers the whole request, streamed or not: a provider that never answers,
/// or keeps streaming, is dropped when the run's time is up. Every attempt, including a
/// retry, records its usage, so retries count towards the monthly budget.
pub(crate) async fn complete(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    req: LlmRequest,
    deadline: Timestamp,
) -> Result<LlmResponse, LlmError> {
    let left = deadline.duration_since(shared.deps.clock.now());
    if left <= SignedDuration::ZERO {
        return Err(LlmError::Unavailable(
            "the run deadline passed before the model was asked; raise [triage] per_run.seconds"
                .to_owned(),
        ));
    }
    let response = tokio::time::timeout(left.unsigned_abs(), provider.complete(req))
        .await
        .map_err(|_| {
            LlmError::Unavailable(
                "the run deadline passed while the model was answering; \
                 raise [triage] per_run.seconds"
                    .to_owned(),
            )
        })??;
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
///
/// A turn that runs out of output room is incomplete: none of it is kept and none of its
/// tool calls are run, whether it held text, a call, both or neither. It is asked again
/// with a larger allowance, within [`MAX_OUTPUT_ALLOWANCE`], the run's remaining output
/// budget and `deadline`.
pub(crate) async fn converse(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    brief: Brief<'_>,
    messages: &mut Vec<LlmMessage>,
    used: &mut Vec<ToolUse>,
    run: &mut RunBudget,
) -> Result<String, LlmError> {
    let budget = shared.settings.budget;
    let specs = if provider.supports_tools() {
        read_tools()
    } else {
        Vec::new()
    };
    let mut calls_used: u16 = 0;
    let mut told_to_answer = false;
    let mut wanted = run.per_turn();
    loop {
        let allowance = run.grant_or_fail(wanted)?;
        let req = LlmRequest {
            system: brief.system.to_owned(),
            messages: messages.clone(),
            tools: specs.clone(),
            max_output_tokens: allowance,
        };
        let response = complete(shared, provider, req, run.deadline()).await?;
        // Checked before anything in the turn is used: a cut-off tool call is as incomplete
        // as cut-off text, and running it would act on half an answer.
        if response.stop == StopReason::MaxTokens {
            match grown_allowance(allowance) {
                Some(larger) => {
                    tracing::info!(
                        provider = provider.id(),
                        from = allowance,
                        to = larger,
                        "the model ran out of output room; retrying the turn with more",
                    );
                    wanted = larger;
                    continue;
                }
                None => {
                    return Err(LlmError::Unavailable(format!(
                        "the model ran out of its {allowance}-token output allowance before \
                         finishing its answer; raise [triage] per_run.output_tokens"
                    )));
                }
            }
        }
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
                tools::call(shared, brief.scope, brief.skills, &name, input).await
            };
            let (tool, used_skill, raw) = target_of(&name, skill, brief.skills);
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
        if calls_used >= budget.max_tool_calls || shared.deps.clock.now() >= run.deadline() {
            results.push(Part::Text(BUDGET_EXHAUSTED.to_owned()));
            told_to_answer = true;
        }
        messages.push(LlmMessage {
            role: Role::User,
            parts: results,
        });
    }
}
