//! The bounded conversation with one provider: turns until the model stops calling tools
//! or the tool-call or time budget runs out.

use std::sync::Arc;

use domovoi_core::ports::{
    LlmCapability, LlmError, LlmMessage, LlmProvider, LlmRequest, LlmResponse, Part, Role,
    StopReason, ToolSpec,
};
use domovoi_core::{ReadTool, SkillSummary};
use jiff::{SignedDuration, Timestamp};

use super::{Shared, tools};

pub(crate) use super::complete::complete;

/// Sent once the budget is spent, with the last tool results.
pub const BUDGET_EXHAUSTED: &str = "Budget exhausted. Reply now with the JSON report.";

/// What a tool call gets once no calls are left.
pub const NO_CALLS_LEFT: &str = "error: the tool-call budget is spent; answer now";

/// How much a turn's output allowance grows when a model runs out of room before it
/// finishes. A reasoning model counts its reasoning against the allowance, so the
/// configured size can be too small for it to say anything at all.
const GROWN_OUTPUT_ALLOWANCE: u32 = 4;

/// The most one turn may ask a provider for. Configuration above this is clamped to it, so
/// the documented maximum holds on every path, the first request included.
pub const MAX_OUTPUT_ALLOWANCE: u32 = 16_000;

/// The allowance a retried turn asks for, or `None` when it is already at
/// [`MAX_OUTPUT_ALLOWANCE`].
pub(crate) fn grown_allowance(current: u32) -> Option<u32> {
    let next = current
        .saturating_mul(GROWN_OUTPUT_ALLOWANCE)
        .min(MAX_OUTPUT_ALLOWANCE);
    (next > current).then_some(next)
}

/// Every request one turn makes if it grows all the way to the ceiling: the first attempt
/// and each retry.
#[derive(Debug, Clone, Copy)]
struct Ladder {
    /// All requests of the sequence, first attempt included.
    total: u32,
    /// Only the retries, which are charged on top of a normal turn.
    retries: u32,
}

/// The growth ladder of one turn that keeps running out of room.
fn growth_ladder(per_turn: u32) -> Ladder {
    let mut ladder = Ladder {
        total: per_turn,
        retries: 0,
    };
    let mut current = per_turn;
    while let Some(next) = grown_allowance(current) {
        ladder.total = ladder.total.saturating_add(next);
        ladder.retries = ladder.retries.saturating_add(next);
        current = next;
    }
    ladder
}

/// The output tokens a run needs to do everything it may legitimately do: every normal turn
/// (`tool_calls + 1` requests), one turn's complete growth ladder, and the report repair
/// with its own complete ladder. This is the default `[triage] per_run.total_tokens`.
///
/// A ladder is charged as its first attempt plus each retry, so a complete 2,000 → 8,000 →
/// 16,000 sequence costs 26,000, not 16,000.
#[must_use]
pub fn default_total_output_tokens(per_turn: u32, tool_calls: u16) -> u32 {
    let per_turn = per_turn.min(MAX_OUTPUT_ALLOWANCE);
    let normal = per_turn.saturating_mul(u32::from(tool_calls).saturating_add(1));
    let ladder = growth_ladder(per_turn);
    normal
        .saturating_add(ladder.retries)
        .saturating_add(ladder.total)
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
            per_turn: shared
                .settings
                .budget
                .max_output_tokens
                .min(MAX_OUTPUT_ALLOWANCE),
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

    /// Gives back a grant for a request the provider refused outright, so a capability
    /// retry does not spend the run's budget twice.
    pub(crate) fn refund(&mut self, granted: u32) {
        self.remaining = self.remaining.saturating_add(granted);
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

/// The failure a run ends with when its time is gone.
pub(crate) fn deadline_error() -> LlmError {
    LlmError::Unavailable(
        "the run deadline passed before the model finished; raise [triage] per_run.seconds"
            .to_owned(),
    )
}

/// The failure for a turn that ran out of output room. Reaching this always means the
/// effective allowance hit the supported maximum, so raising the setting is not offered.
fn exhausted() -> LlmError {
    LlmError::Unavailable(format!(
        "the model ran out of the supported maximum of {MAX_OUTPUT_ALLOWANCE} output tokens \
         before finishing its answer; [triage] per_run.output_tokens cannot raise it further"
    ))
}

/// One model request that must end in a complete answer.
///
/// This is the only place an answer is accepted. A turn that ends at its output allowance is
/// incomplete whatever it holds: nothing generated is returned, no tool call in it runs and
/// no message is added to `messages`. The turn is asked again with a larger allowance, within
/// [`MAX_OUTPUT_ALLOWANCE`], the run's remaining budget and its deadline. A provider that
/// refuses streamed usage is retried once, which the adapter answers as a whole response
/// carrying usage.
pub(crate) async fn ask(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    system: &str,
    messages: &[LlmMessage],
    tools: Vec<ToolSpec>,
    run: &mut RunBudget,
) -> Result<LlmResponse, LlmError> {
    let mut wanted = run.per_turn();
    let mut usage_retried = false;
    loop {
        let allowance = run.grant_or_fail(wanted)?;
        let req = LlmRequest {
            system: system.to_owned(),
            messages: messages.to_vec(),
            tools: tools.clone(),
            max_output_tokens: allowance,
        };
        match complete(shared, provider, req, run.deadline()).await {
            Ok(response) if response.stop == StopReason::MaxTokens => {
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
                    None => return Err(exhausted()),
                }
            }
            Ok(response) => return Ok(response),
            Err(LlmError::CapabilityRejected {
                capability: LlmCapability::StreamUsage,
                ..
            }) if !usage_retried => {
                // The adapter has stopped asking for streamed usage and will answer with a
                // whole response instead. Nothing was generated, so the grant goes back.
                usage_retried = true;
                run.refund(allowance);
                continue;
            }
            Err(
                err @ LlmError::CapabilityRejected {
                    capability: LlmCapability::Tools,
                    ..
                },
            ) => {
                // A definite refusal before anything was generated. The caller rebuilds the
                // attempt without tools, and that rebuild must have the allowance back.
                run.refund(allowance);
                return Err(err);
            }
            Err(err) => return Err(err),
        }
    }
}

/// Continues `messages` with `provider` until the model answers without tool calls, and
/// returns that answer's text. Tool calls are answered through [`tools::call`] within the
/// run budget; once it is spent the model is told to answer and further calls are not
/// run. Every call the model makes while it may still call tools is pushed onto `used`;
/// calls in the response after it was told to answer are ignored.
///
/// Answers come from [`ask`], so an incomplete turn is never accepted, kept or acted on.
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
    loop {
        let response = ask(shared, provider, brief.system, messages, specs.clone(), run).await?;
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
            // Tools stop at the run deadline, not only at their own timeout: a batch of
            // individually quick calls must not run past the run's time.
            if shared.deps.clock.now() >= run.deadline() {
                return Err(deadline_error());
            }
            let skill = read_skill_of(&name, &input);
            let refused = calls_used >= budget.max_tool_calls;
            let result = if refused {
                tools::ToolResult {
                    content: NO_CALLS_LEFT.to_owned(),
                    is_error: true,
                }
            } else {
                calls_used += 1;
                tools::call(shared, brief.scope, &name, input, run.deadline()).await
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
        if shared.deps.clock.now() >= run.deadline() {
            return Err(deadline_error());
        }
        if calls_used >= budget.max_tool_calls {
            results.push(Part::Text(BUDGET_EXHAUSTED.to_owned()));
            told_to_answer = true;
        }
        messages.push(LlmMessage {
            role: Role::User,
            parts: results,
        });
    }
}
