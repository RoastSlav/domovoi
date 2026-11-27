//! The bounded conversation with one provider over the meta-tools.

use domovoi_core::ports::{LlmError, LlmMessage, Part, Role, ToolPhase};
use serde_json::Value;

use super::dispatch::{Handled, State, handle};
use super::finish::{Carrier, accept, asked};
use super::{Ctx, EvidenceRecord, ExperienceAccess, LoopOutput, LoopScope, offered_tools};
use crate::triage::agent::{self, BUDGET_EXHAUSTED, RunBudget, ToolUse, deadline_error, text_of};
use crate::triage::chain::AttemptError;
use crate::triage::report::parse_report;

/// What the model is told to do after an answer that could not be used.
const HOW_TO_ANSWER: &str = "Reply with only the JSON object described in your instructions.";

/// What the model is told after a `finish` whose report did not validate.
const FINISH_AGAIN: &str = "Your answer could not be used: call finish again with a valid report.";

/// The answer when the model used every read it was given and wrote nothing. It says why and
/// where the limit is, so the person is not left with an empty message.
const NO_ANSWER_IN_BUDGET: &str = "I used all the reads I am allowed for one question and did not reach an answer. Ask again with a narrower question, or raise the tool-call limit (Settings, Triage, Tool calls per run).";

fn tool_calls(parts: &[Part]) -> Vec<(String, String, Value)> {
    parts
        .iter()
        .filter_map(|p| match p {
            Part::ToolCall { id, name, input } => Some((id.clone(), name.clone(), input.clone())),
            _ => None,
        })
        .collect()
}

/// Spends the run's one repair, or ends the attempt when it is already spent.
fn repair(repairs_left: &mut u8, why: &str) -> Result<(), AttemptError> {
    if *repairs_left == 0 {
        return Err(AttemptError::Invalid(format!(
            "the model gave no usable report: {why}"
        )));
    }
    *repairs_left -= 1;
    Ok(())
}

/// Runs one provider's investigation until it delivers a report or asks a question.
///
/// Every meta-tool call but `finish` counts against the run's tool-call budget; once it is
/// spent the model is told to answer and further calls are refused. A text answer is read as
/// a report (an implicit `finish`). An unusable answer or `finish` gets one repair per
/// attempt. Every parsed read is pushed onto `evidence` as it happens, so reads made before a
/// failure stay recorded.
pub(crate) async fn investigate(
    ctx: &Ctx<'_>,
    scope: &LoopScope,
    messages: &mut Vec<LlmMessage>,
    used: &mut Vec<ToolUse>,
    evidence: &mut Vec<EvidenceRecord>,
    run: &mut RunBudget,
) -> Result<LoopOutput, AttemptError> {
    let shared = ctx.shared;
    let offered = ctx.provider.supports_tools();
    let specs = if offered {
        offered_tools(scope.tools)
    } else {
        Vec::new()
    };
    let mut state = State::investigation(scope);
    let mut repairs_left: u8 = 1;
    loop {
        let response = agent::ask(
            shared,
            ctx.provider,
            ctx.system,
            messages,
            specs.clone(),
            run,
        )
        .await?;
        messages.push(LlmMessage {
            role: Role::Assistant,
            parts: response.parts.clone(),
        });
        let calls = tool_calls(&response.parts);
        let text = text_of(&response.parts);
        if calls.is_empty() || !offered {
            match parse_report(&text) {
                Ok(report) => return Ok(accept(ctx, state, report, Carrier::Text).await),
                Err(why) => {
                    let why = shared.deps.redactor.redact(&why);
                    repair(&mut repairs_left, &why)?;
                    messages.push(LlmMessage {
                        role: Role::User,
                        parts: vec![Part::Text(format!(
                            "Your answer could not be used: {why}. {HOW_TO_ANSWER}"
                        ))],
                    });
                    continue;
                }
            }
        }
        let told_before = state.told;
        let mut results = Vec::with_capacity(calls.len() + 1);
        for (id, name, input) in calls {
            // Calls stop at the run deadline, not only at their own timeout: a batch of
            // individually quick calls must not run past the run's time.
            if shared.deps.clock.now() >= run.deadline() {
                return Err(deadline_error().into());
            }
            let handled = handle(
                ctx,
                &mut state,
                &name,
                input,
                used,
                evidence,
                run.deadline(),
            )
            .await;
            let result = match handled {
                Handled::Result(result) => result,
                Handled::Finish(report) => {
                    // Calls after `finish` in the same response are ignored.
                    return Ok(accept(ctx, state, *report, Carrier::Finish).await);
                }
                Handled::Asked(question) => return Ok(asked(state, question)),
                Handled::FinishRejected(why) => {
                    repair(&mut repairs_left, &why)?;
                    crate::triage::tools::ToolResult {
                        content: format!("finish rejected: {why}. {FINISH_AGAIN}"),
                        is_error: true,
                    }
                }
            };
            results.push(Part::ToolResult {
                id,
                content: result.content,
                is_error: result.is_error,
            });
        }
        if shared.deps.clock.now() >= run.deadline() {
            return Err(deadline_error().into());
        }
        if told_before {
            // The model was told to answer and called tools instead. Its text is read as the
            // answer, and the one repair is spent when it is not a report.
            match parse_report(&text) {
                Ok(report) => return Ok(accept(ctx, state, report, Carrier::Text).await),
                Err(why) => {
                    let why = shared.deps.redactor.redact(&why);
                    repair(&mut repairs_left, &why)?;
                    results.push(Part::Text(format!(
                        "Your answer could not be used: {why}. {HOW_TO_ANSWER}"
                    )));
                }
            }
        }
        if state.calls_used >= shared.settings.budget.max_tool_calls && !state.told {
            results.push(Part::Text(BUDGET_EXHAUSTED.to_owned()));
            state.told = true;
        }
        messages.push(LlmMessage {
            role: Role::User,
            parts: results,
        });
    }
}

/// Continues `messages` with the read-only tools until the model answers without tool
/// calls, and returns that answer's text. Once the tool-call budget is spent the model is
/// told to answer, and the next response's text is the answer whatever it holds. Every call
/// the model makes while it may still call tools is pushed onto `used`.
pub(crate) async fn converse_read(
    ctx: &Ctx<'_>,
    messages: &mut Vec<LlmMessage>,
    used: &mut Vec<ToolUse>,
    run: &mut RunBudget,
) -> Result<String, LlmError> {
    let shared = ctx.shared;
    let mut state = State::read_only(ExperienceAccess::host(ctx.scope.host, None));
    let specs = if ctx.provider.supports_tools() {
        offered_tools(state.tools)
    } else {
        Vec::new()
    };
    // Reads in a conversation are not recorded as evidence steps: there is no operation.
    let mut evidence: Vec<EvidenceRecord> = Vec::new();
    loop {
        let response = agent::ask(
            shared,
            ctx.provider,
            ctx.system,
            messages,
            specs.clone(),
            run,
        )
        .await?;
        messages.push(LlmMessage {
            role: Role::Assistant,
            parts: response.parts.clone(),
        });
        let calls = tool_calls(&response.parts);
        if calls.is_empty() || state.told || specs.is_empty() {
            let text = text_of(&response.parts);
            if text.trim().is_empty() && !calls.is_empty() {
                return Ok(NO_ANSWER_IN_BUDGET.to_owned());
            }
            return Ok(text);
        }
        let mut results = Vec::with_capacity(calls.len() + 1);
        for (id, name, input) in calls {
            if shared.deps.clock.now() >= run.deadline() {
                return Err(deadline_error());
            }
            let before = used.len();
            if let Some(sink) = run.sink() {
                sink.tool(&name, ToolPhase::Started, false);
            }
            let handled = handle(
                ctx,
                &mut state,
                &name,
                input,
                used,
                &mut evidence,
                run.deadline(),
            )
            .await;
            if let Some(sink) = run.sink() {
                // The call as it was recorded: the closed name the engine matched, not the
                // model's text.
                match used.get(before..).and_then(<[ToolUse]>::last) {
                    Some(call) => {
                        sink.tool(&call.tool, ToolPhase::Finished, call.failed || call.refused)
                    }
                    None => sink.tool(&name, ToolPhase::Finished, true),
                }
            }
            // `finish` and `ask_user` are never offered here, so they never get this far.
            let Handled::Result(result) = handled else {
                continue;
            };
            results.push(Part::ToolResult {
                id,
                content: result.content,
                is_error: result.is_error,
            });
        }
        if shared.deps.clock.now() >= run.deadline() {
            return Err(deadline_error());
        }
        if state.calls_used >= shared.settings.budget.max_tool_calls {
            results.push(Part::Text(BUDGET_EXHAUSTED.to_owned()));
            state.told = true;
        }
        messages.push(LlmMessage {
            role: Role::User,
            parts: results,
        });
    }
}
