//! One meta-tool call: parsed, checked against what is offered and the budgets, then run.

use domovoi_core::meta::{MetaArgError, MetaTool, MetaToolCall, PlanInput, PlanStepInput, ToolSet};
use domovoi_core::{ReadTool, TriageReport};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::{
    Ctx, EvidenceRecord, LoopScope, MAX_QUESTIONS_PER_OPERATION, capabilities, evidence, record,
};
use crate::plan::{PlanScope, PlanValidator, PlannedStep};
use crate::triage::agent::{NO_CALLS_LEFT, ToolUse};
use crate::triage::tools::{ToolResult, finish};

/// Longest question put to the person who runs the server, in characters.
const QUESTION_CHARS: usize = 500;

/// One refusal the loop logged, kept to work out which were never corrected.
pub(super) struct Logged {
    /// The "Proposals not offered" line.
    pub(super) line: String,
    /// The capability the model named.
    pub(super) contract: String,
    /// Whether the step already exists, so it is not lost.
    pub(super) duplicate: bool,
}

/// What one conversation's calls have used and staged so far. Every provider attempt starts
/// with a fresh one.
pub(crate) struct State<'a> {
    pub(super) tools: ToolSet,
    pub(super) plan: Option<&'a PlanScope>,
    pub(super) calls_used: u16,
    pub(super) told: bool,
    pub(super) staged: Vec<PlannedStep>,
    pub(super) refusals: Vec<Logged>,
    pub(super) evidence_left: Option<u32>,
    pub(super) questions_asked: u32,
}

impl<'a> State<'a> {
    /// The state of an investigation in `scope`.
    pub(super) fn investigation(scope: &'a LoopScope) -> Self {
        Self {
            tools: scope.tools,
            plan: Some(&scope.plan),
            calls_used: 0,
            told: false,
            staged: Vec::new(),
            refusals: Vec::new(),
            evidence_left: scope.evidence_left,
            questions_asked: scope.questions_asked,
        }
    }

    /// The state of a follow-up or chat: look, never suggest.
    pub(super) fn read_only() -> Self {
        Self {
            tools: ToolSet::Read,
            plan: None,
            calls_used: 0,
            told: false,
            staged: Vec::new(),
            refusals: Vec::new(),
            evidence_left: None,
            questions_asked: 0,
        }
    }
}

/// What a call came to.
pub(super) enum Handled {
    /// A result to hand back to the model.
    Result(ToolResult),
    /// A valid `finish`.
    Finish(Box<TriageReport>),
    /// A `finish` whose report did not validate, with why.
    FinishRejected(String),
    /// An accepted question.
    Asked(String),
}

fn error(ctx: &Ctx<'_>, text: &str) -> ToolResult {
    finish(&ctx.shared.deps, text, true)
}

/// Handles one call to `name` with `input`, records it in `used` and every parsed read in
/// `records`, and says what it came to.
pub(super) async fn handle(
    ctx: &Ctx<'_>,
    state: &mut State<'_>,
    name: &str,
    input: Value,
    used: &mut Vec<ToolUse>,
    records: &mut Vec<EvidenceRecord>,
    deadline: Timestamp,
) -> Handled {
    let tool = MetaTool::from_name(name);
    if tool.is_some_and(|tool| !state.tools.offers(tool)) {
        used.push(record::unknown(name, false));
        return Handled::Result(error(ctx, &format!("error: unknown tool `{name}`")));
    }
    let call = match MetaToolCall::from_json(name, input.clone()) {
        Ok(call) => call,
        Err(err) => {
            if tool == Some(MetaTool::Finish) {
                return Handled::FinishRejected(ctx.shared.deps.redactor.redact(&err.to_string()));
            }
            used.push(record::recorded(name, &input, ctx.skills, true, false));
            let mut text = match &err {
                // A read's own argument error reads exactly as it always did.
                MetaArgError::InvalidArgs { tool, reason }
                    if *tool == MetaTool::RequestEvidence.name() =>
                {
                    format!("error: {reason}")
                }
                other => format!("error: {other}"),
            };
            if matches!(err, MetaArgError::UnknownTool(_)) && ReadTool::from_name(name).is_some() {
                text.push_str(&format!("\nuse request_evidence with tool \"{name}\""));
            }
            return Handled::Result(error(ctx, &text));
        }
    };
    let call = match call {
        MetaToolCall::Finish(report) => return Handled::Finish(report),
        other => other,
    };
    let max = ctx.shared.settings.budget.max_tool_calls;
    if state.told || state.calls_used >= max {
        used.push(record::recorded(name, &input, ctx.skills, false, true));
        if let MetaToolCall::RequestEvidence(read) = call {
            records.push(evidence::refused(read, evidence::CALLS_SPENT));
        }
        return Handled::Result(ToolResult {
            content: NO_CALLS_LEFT.to_owned(),
            is_error: true,
        });
    }
    state.calls_used += 1;
    let (result, asked) = match call {
        MetaToolCall::ListCapabilities {
            effect,
            include_unavailable,
        } => (
            capabilities::list(ctx, effect, include_unavailable).await,
            None,
        ),
        MetaToolCall::DescribeCapability { contract } => {
            (capabilities::describe(ctx, &contract).await, None)
        }
        MetaToolCall::RequestEvidence(read) => (
            evidence::request(ctx, state, read, deadline, records).await,
            None,
        ),
        MetaToolCall::ProposePlan(plan) => (propose(ctx, state, &plan).await, None),
        MetaToolCall::AskUser { question } => ask(ctx, state, &question),
        MetaToolCall::Finish(report) => return Handled::Finish(report),
    };
    used.push(record::recorded(
        name,
        &input,
        ctx.skills,
        result.is_error,
        false,
    ));
    match asked {
        Some(question) => Handled::Asked(question),
        None => Handled::Result(result),
    }
}

/// `ask_user`: accepted only before any step is staged and while questions remain.
fn ask(ctx: &Ctx<'_>, state: &State<'_>, question: &str) -> (ToolResult, Option<String>) {
    if !state.staged.is_empty() {
        return (
            error(ctx, "error: you already proposed steps; call finish"),
            None,
        );
    }
    if state.questions_asked >= MAX_QUESTIONS_PER_OPERATION {
        return (
            error(
                ctx,
                "error: you have asked enough; finish with what you know",
            ),
            None,
        );
    }
    let text: String = ctx
        .shared
        .deps
        .redactor
        .redact(question)
        .chars()
        .take(QUESTION_CHARS)
        .collect();
    (
        ToolResult {
            content: "The question was put to the person who runs the server.".to_owned(),
            is_error: false,
        },
        Some(text),
    )
}

/// Validates `input` against the plan scope and the steps staged so far, stages what passes
/// and logs what does not.
pub(super) async fn validate(ctx: &Ctx<'_>, state: &mut State<'_>, input: &PlanInput) -> Value {
    let deps = &ctx.shared.deps;
    let Some(scope) = state.plan else {
        return json!({"accepted": [], "refused": []});
    };
    let validator = PlanValidator {
        registry: deps.capabilities.as_ref(),
        resolver: deps.resolver.as_ref(),
        hosts: deps.hosts.as_ref(),
        redactor: deps.redactor.as_ref(),
    };
    let verdict = validator.validate(scope, &state.staged, input).await;
    let mut accepted = Vec::new();
    for (index, step) in verdict.accepted.iter().enumerate() {
        let host = match deps.hosts.get_host(step.host).await {
            Ok(Some(found)) => found.name.to_string(),
            _ => step.host.to_string(),
        };
        let mut entry =
            json!({"index": index, "contract": step.contract.to_string(), "host": host});
        if let Some(note) = &step.note {
            entry["note"] = json!(deps.redactor.redact(note));
        }
        accepted.push(entry);
    }
    let mut refused = Vec::new();
    for refusal in &verdict.refused {
        let mut entry = json!({
            "index": refusal.index,
            "contract": refusal.contract,
            "code": refusal.code.as_str(),
            "message": refusal.message,
        });
        if let Some(hint) = &refusal.hint {
            entry["hint"] = json!(hint);
        }
        refused.push(entry);
        state.refusals.push(Logged {
            line: deps.redactor.redact(&refusal.dropped_line()),
            contract: input.steps.get(refusal.index).map_or_else(
                || refusal.contract.clone(),
                |step: &PlanStepInput| step.contract.clone(),
            ),
            duplicate: refusal.code.is_duplicate(),
        });
    }
    // Accepted steps' indexes are positions in the call, not in `accepted`.
    let positions: Vec<usize> = (0..input.steps.len())
        .filter(|i| !verdict.refused.iter().any(|r| r.index == *i))
        .collect();
    for (entry, position) in accepted.iter_mut().zip(positions) {
        entry["index"] = json!(position);
    }
    state.staged.extend(verdict.accepted);
    json!({"accepted": accepted, "refused": refused})
}

/// `propose_plan`: validates and stages; the result lists every accepted and refused step.
async fn propose(ctx: &Ctx<'_>, state: &mut State<'_>, input: &PlanInput) -> ToolResult {
    let mut verdict = validate(ctx, state, input).await;
    let nothing = verdict["accepted"].as_array().is_none_or(Vec::is_empty);
    let something_refused = verdict["refused"].as_array().is_some_and(|r| !r.is_empty());
    verdict["note"] = json!(
        "Accepted steps are suggestions. A person approves or ignores them; nothing has run."
    );
    finish(
        &ctx.shared.deps,
        &verdict.to_string(),
        nothing && something_refused,
    )
}

/// The capability a contract text names, the `skill.` prefix ignored, for matching a refusal
/// with a later staged step.
pub(super) fn family_name(contract: &str) -> &str {
    contract.strip_prefix("skill.").unwrap_or(contract)
}
