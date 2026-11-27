//! Events about steps the writer does not own: proposing, starting, finishing, answering.

use domovoi_core::ports::EffectKind;
use domovoi_core::{
    BudgetKind, EdgeKind, PauseReason, StepId, StepIntent, StepKind, StepOutcome, StepProgress,
    StepUsage,
};

use serde_json::json;

use super::DecideError;
use super::effects::{
    COLLECT_ATTEMPTS, WAKE_ATTEMPTS, collect_key, question_key, step_payload, wake_payload,
};
use super::events::{NewEdge, NewStep};
use super::status::{Allow, gate, gate_settle};
use super::turn::{Resume, resume};
use super::work::Work;
use crate::operations::audit;

/// Kinds a caller may propose: the writer owns approvals, executions, verifications and
/// policy decisions.
const fn is_proposable(kind: StepKind) -> bool {
    !matches!(
        kind,
        StepKind::Execution
            | StepKind::Approval
            | StepKind::Verification
            | StepKind::PolicyDecision
    )
}

/// The progress of a step that is born already over.
fn born_final(work: &Work<'_>, outcome: StepOutcome, reason: Option<&str>) -> StepProgress {
    let started_at = (outcome == StepOutcome::Succeeded).then_some(work.now);
    StepProgress {
        outcome,
        started_at,
        ended_at: Some(work.now),
        summary: None,
        reason: reason.map(str::to_owned),
        output: None,
    }
}

/// Adds the steps, then the edges.
pub(super) fn propose(
    work: &mut Work<'_>,
    steps: Vec<NewStep>,
    edges: Vec<NewEdge>,
) -> Result<(), DecideError> {
    gate(work.op(), Allow::QUEUED_ACTIVE)?;
    for step in &steps {
        for kind in [step.kind, step.intent.kind()] {
            if !is_proposable(kind) {
                return Err(DecideError::KindNotProposable(kind));
            }
        }
    }
    for step in steps {
        born(work, step)?;
    }
    for edge in edges {
        work.add_edge(edge.from, edge.to, edge.kind)?;
    }
    Ok(())
}

/// Adds one proposed step in the outcome its kind is born with, and what that kind sets
/// going.
fn born(work: &mut Work<'_>, new: NewStep) -> Result<(), DecideError> {
    let NewStep {
        id,
        phase,
        parent,
        intent,
        ..
    } = new;
    let operation = work.op().id;
    let cycle = work.op().cycle;
    match &intent {
        StepIntent::ReasoningTurn { .. } => {
            if work.op().budgets.is_spent(BudgetKind::ModelTurns, cycle) {
                let refused =
                    born_final(work, StepOutcome::Refused, Some("model turn budget spent"));
                work.add_step(id, &phase, parent, intent, refused)?;
                work.pause(PauseReason::Budget {
                    exhausted: BudgetKind::ModelTurns,
                });
            } else {
                let running = StepProgress {
                    outcome: StepOutcome::Running,
                    started_at: Some(work.now),
                    ..StepProgress::pending()
                };
                work.add_step(id, &phase, parent, intent, running)?;
                work.charge(BudgetKind::ModelTurns, 1);
            }
        }
        StepIntent::EvidenceRequest { .. } => {
            if work
                .op()
                .budgets
                .is_spent(BudgetKind::EvidenceRequests, cycle)
            {
                let refused = born_final(work, StepOutcome::Refused, Some("evidence budget spent"));
                work.add_step(id, &phase, parent, intent, refused)?;
                work.pause(PauseReason::Budget {
                    exhausted: BudgetKind::EvidenceRequests,
                });
            } else {
                work.add_step(id, &phase, parent, intent, StepProgress::pending())?;
                work.charge(BudgetKind::EvidenceRequests, 1);
                let now = work.now;
                work.effect(
                    EffectKind::CollectEvidence,
                    step_payload(operation, id),
                    now,
                    COLLECT_ATTEMPTS,
                    collect_key(id),
                );
            }
        }
        StepIntent::Question { expires_at, text } => {
            let expires_at = *expires_at;
            // The question itself is the step's summary, so every view of the step (trace,
            // Attention, the incident page) can show what is being asked of the person.
            let mut asked = StepProgress::pending();
            asked.summary = Some(text.chars().take(280).collect());
            work.add_step(id, &phase, parent, intent, asked)?;
            work.effect(
                EffectKind::Wake,
                wake_payload(operation, Some(id), "question_timeout"),
                expires_at,
                WAKE_ATTEMPTS,
                question_key(id),
            );
        }
        StepIntent::Notification { .. } => {
            work.add_step(id, &phase, parent, intent, StepProgress::pending())?;
        }
        StepIntent::Assessment { .. } | StepIntent::Plan { .. } | StepIntent::Note { .. } => {
            let done = born_final(work, StepOutcome::Succeeded, None);
            work.add_step(id, &phase, parent, intent, done)?;
        }
        other => return Err(DecideError::KindNotProposable(other.kind())),
    }
    Ok(())
}

/// A pending step starts. A step that is already running is left alone.
pub(super) fn started(work: &mut Work<'_>, step: StepId) -> Result<(), DecideError> {
    let record = work.find(step)?;
    let (outcome, kind) = (record.progress.outcome, record.step.kind);
    let progress = record.progress.clone();
    match outcome {
        StepOutcome::Running => return Ok(()),
        StepOutcome::Pending => {}
        _ => return Err(DecideError::StepNotOpen(step)),
    }
    gate(work.op(), Allow::ACTIVE)?;
    if matches!(kind, StepKind::Execution | StepKind::Approval) {
        return Err(DecideError::Invalid(
            "approvals and executions change only through their own events".into(),
        ));
    }
    let running = StepProgress {
        outcome: StepOutcome::Running,
        started_at: Some(work.now),
        ..progress
    };
    work.set_progress(step, running)
}

/// A step reaches a final outcome. A model turn also charges the tokens and cost it reports;
/// the turn itself was counted when it was accepted.
pub(super) fn finished(
    work: &mut Work<'_>,
    step: StepId,
    outcome: StepOutcome,
    summary: Option<String>,
    reason: Option<String>,
    output: Option<String>,
    usage: Option<StepUsage>,
) -> Result<(), DecideError> {
    let record = work.find(step)?;
    let (before, kind) = (record.progress.outcome, record.step.kind);
    let progress = record.progress.clone();
    if before.is_final() {
        return Err(DecideError::StepNotOpen(step));
    }
    let record_only = gate_settle(work.op(), before == StepOutcome::Running, true)?;
    if matches!(
        kind,
        StepKind::Execution | StepKind::Approval | StepKind::Verification
    ) {
        return Err(DecideError::Invalid(
            "approvals, executions and verifications change only through their own events".into(),
        ));
    }
    if !outcome.is_final() {
        return Err(DecideError::Invalid(
            "a step finishes with a final outcome".into(),
        ));
    }
    let ended = work.ended(
        &progress,
        outcome,
        summary.as_deref(),
        reason.as_deref(),
        output.as_deref(),
    );
    work.set_progress(step, ended)?;
    if kind == StepKind::ReasoningTurn
        && let Some(used) = usage
    {
        let tokens = u64::from(used.input_tokens) + u64::from(used.output_tokens);
        work.charge(BudgetKind::Tokens, tokens);
        work.charge(BudgetKind::Cost, used.cost_micros.unwrap_or(0));
    }
    if !record_only && let Some(exhausted) = work.op().budgets.overrun() {
        work.pause(PauseReason::Budget { exhausted });
    }
    if kind == StepKind::Question && outcome == StepOutcome::NotRun && !record_only {
        resume(work, step, Resume::Timeout);
    }
    Ok(())
}

/// A person answers a question: a Note step answers it and the question succeeds.
pub(super) fn answer(
    work: &mut Work<'_>,
    question: StepId,
    note: NewStep,
) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE)?;
    let record = work.find(question)?;
    if record.step.kind != StepKind::Question {
        return Err(DecideError::Invalid(
            "only a question can be answered".into(),
        ));
    }
    if record.progress.outcome.is_final() {
        return Err(DecideError::StepNotOpen(question));
    }
    let progress = record.progress.clone();
    if note.kind != StepKind::Note || note.intent.kind() != StepKind::Note {
        return Err(DecideError::Invalid("an answer is a note".into()));
    }
    let NewStep {
        id,
        phase,
        parent,
        intent,
        ..
    } = note;
    let done = born_final(work, StepOutcome::Succeeded, None);
    work.add_step(id, &phase, parent, intent, done)?;
    work.add_edge(id, question, EdgeKind::Answers)?;
    let answered = StepProgress {
        started_at: progress.started_at.or(Some(work.now)),
        ..work.ended(&progress, StepOutcome::Succeeded, None, None, None)
    };
    work.set_progress(question, answered)?;
    work.audit_operation(audit::OPERATION_ANSWERED, Some(question), json!({}));
    resume(work, question, Resume::Answer);
    Ok(())
}
