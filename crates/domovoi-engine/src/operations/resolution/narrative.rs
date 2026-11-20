//! The narrative of a summary, taken from text the model already wrote. Nothing is fetched
//! and no model is called.

use domovoi_core::{OperationSnapshot, StepIntent, StepKind, StepOutcome};

use crate::operations::investigate::{TurnKind, TurnOutput};

/// The drafted text of the three narrative fields; a field without text has no draft.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Narrative {
    /// What was wrong.
    pub(crate) problem: Option<String>,
    /// Why.
    pub(crate) cause: Option<String>,
    /// What fixes it.
    pub(crate) fix: Option<String>,
}

/// The newest successful turn's report, else the immutable assessment and plan texts. `None`
/// when neither exists.
pub(crate) fn draft_narrative(snapshot: &OperationSnapshot) -> Option<Narrative> {
    from_report(snapshot).or_else(|| from_steps(snapshot))
}

fn from_report(snapshot: &OperationSnapshot) -> Option<Narrative> {
    let record = snapshot
        .steps
        .iter()
        .filter(|record| {
            record.step.kind == StepKind::ReasoningTurn
                && record.progress.outcome == StepOutcome::Succeeded
        })
        .filter(|record| report_of(record.progress.output.as_deref()).is_some())
        .max_by_key(|record| record.step.ordinal)?;
    let report = report_of(record.progress.output.as_deref())?;
    let mut fix = report.next_step.clone();
    if !report.manual_steps.is_empty() {
        fix.push('\n');
        fix.push_str(&report.manual_steps.join("\n"));
    }
    Some(Narrative {
        problem: Some(report.summary),
        cause: Some(report.cause),
        fix: Some(fix),
    })
}

fn report_of(output: Option<&str>) -> Option<domovoi_core::TriageReport> {
    let parsed: TurnOutput = serde_json::from_str(output?).ok()?;
    matches!(parsed.kind, TurnKind::Report | TurnKind::Cached)
        .then_some(parsed.report)
        .flatten()
}

fn from_steps(snapshot: &OperationSnapshot) -> Option<Narrative> {
    let newest = |kind: StepKind| {
        snapshot
            .steps
            .iter()
            .filter(|record| record.step.kind == kind)
            .max_by_key(|record| record.step.ordinal)
    };
    let assessment = newest(StepKind::Assessment).and_then(|r| match &r.step.intent {
        StepIntent::Assessment { text } => Some(text.as_str()),
        _ => None,
    });
    let plan = newest(StepKind::Plan).and_then(|r| match &r.step.intent {
        StepIntent::Plan { text } => Some(text.clone()),
        _ => None,
    });
    if assessment.is_none() && plan.is_none() {
        return None;
    }
    let (problem, cause) = match assessment {
        Some(text) => match text.split_once('\n') {
            Some((first, rest)) => (Some(first.to_owned()), Some(rest.to_owned())),
            None => (Some(text.to_owned()), None),
        },
        None => (None, None),
    };
    Some(Narrative {
        problem,
        cause,
        fix: plan,
    })
}
