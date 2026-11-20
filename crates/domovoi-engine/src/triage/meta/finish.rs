//! Accepting the report: from `finish` or from a text answer, with its proposals derived from
//! the staged plan.

use domovoi_core::meta::PlanInput;
use domovoi_core::{ActionProposal, TriageReport};

use super::dispatch::{State, family_name, validate};
use super::{Ctx, LoopEnd, LoopOutput};

/// How the report arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Carrier {
    /// Through the `finish` tool, without proposals.
    Finish,
    /// As a text answer, whose proposals still have to be validated.
    Text,
}

/// The loop's output for `report`. A text report's proposals go through the same validator as
/// `propose_plan`; either way the report's proposals are then the staged steps.
pub(super) async fn accept(
    ctx: &Ctx<'_>,
    mut state: State<'_>,
    mut report: TriageReport,
    carrier: Carrier,
) -> LoopOutput {
    if carrier == Carrier::Text
        && let Some(input) = PlanInput::from_proposals(&report.proposals)
    {
        validate(ctx, &mut state, &input).await;
    }
    report.proposals = state
        .staged
        .iter()
        .filter_map(|step| {
            Some(ActionProposal {
                skill: step.contract.skill_name()?,
                args: step.args.clone(),
                reason: step.reason.clone(),
            })
        })
        .collect();
    let dropped = dropped(&state);
    LoopOutput {
        end: LoopEnd::Report(report),
        plan: state.staged,
        dropped,
    }
}

/// The loop's output when the model asked a question.
pub(super) fn asked(state: State<'_>, question: String) -> LoopOutput {
    let dropped = dropped(&state);
    LoopOutput {
        end: LoopEnd::Asked(question),
        plan: state.staged,
        dropped,
    }
}

/// The refusals the model never corrected: a refusal for a capability that a later step
/// staged counts as corrected, and a duplicate is not lost. Each line appears once.
fn dropped(state: &State<'_>) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for logged in &state.refusals {
        if logged.duplicate {
            continue;
        }
        let corrected = state
            .staged
            .iter()
            .any(|step| family_name(&step.contract.to_string()) == family_name(&logged.contract));
        if !corrected && !lines.contains(&logged.line) {
            lines.push(logged.line.clone());
        }
    }
    lines
}
