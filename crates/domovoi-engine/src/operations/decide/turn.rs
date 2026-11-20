//! A model turn that ends together with the reads it made, and the resume of an
//! investigation that waited for an answer.

use domovoi_core::ports::EffectKind;
use domovoi_core::{
    BudgetKind, EdgeKind, OperationKind, StepId, StepIntent, StepKind, StepOutcome, StepProgress,
};

use super::DecideError;
use super::effects::{BUILD_CONTEXT_ATTEMPTS, resume_key, resume_payload};
use super::events::RecordedStep;
use super::status::{Allow, gate};
use super::steps::finished;
use super::work::{Work, cut_output, short_text};

/// The phase every evidence step carries.
const EVIDENCE_PHASE: &str = "evidence";

/// How a recorded read may end.
const fn recordable(outcome: StepOutcome) -> bool {
    matches!(
        outcome,
        StepOutcome::Succeeded
            | StepOutcome::Failed
            | StepOutcome::Unavailable
            | StepOutcome::Refused
    )
}

/// The turn's reads become `EvidenceRequest` steps born final, then the turn ends as a
/// `StepFinished` would end it. No `CollectEvidence` effect is made, and a read never pauses
/// the operation: the loop already honoured the evidence budget.
pub(super) fn turn_finished(
    work: &mut Work<'_>,
    turn: StepId,
    ended: Ending,
    evidence: Vec<RecordedStep>,
) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE_PAUSED)?;
    let record = work.find(turn)?;
    if record.step.kind != StepKind::ReasoningTurn {
        return Err(DecideError::Invalid("not a reasoning turn".into()));
    }
    if record.progress.outcome != StepOutcome::Running {
        return Err(DecideError::StepNotOpen(turn));
    }
    for read in evidence {
        record_read(work, turn, read)?;
    }
    finished(
        work,
        turn,
        ended.outcome,
        ended.summary,
        ended.reason,
        ended.output,
        ended.usage,
    )
}

/// What the turn itself came to.
pub(super) struct Ending {
    pub(super) outcome: StepOutcome,
    pub(super) summary: Option<String>,
    pub(super) reason: Option<String>,
    pub(super) output: Option<String>,
    pub(super) usage: Option<domovoi_core::StepUsage>,
}

/// Adds one read as a final `EvidenceRequest` step caused by `turn`.
fn record_read(work: &mut Work<'_>, turn: StepId, read: RecordedStep) -> Result<(), DecideError> {
    if !recordable(read.outcome) {
        return Err(DecideError::Invalid(
            "a recorded read ends succeeded, failed, unavailable or refused".into(),
        ));
    }
    let now = work.now;
    let progress = StepProgress {
        outcome: read.outcome,
        started_at: Some(now),
        ended_at: Some(now),
        summary: read.summary.as_deref().map(short_text),
        reason: read.reason.as_deref().map(short_text),
        output: read.output.as_deref().map(cut_output),
    };
    let intent = StepIntent::EvidenceRequest {
        host: read.host,
        tool: read.tool,
        args: read.args,
    };
    work.add_step(read.id, EVIDENCE_PHASE, Some(turn), intent, progress)?;
    work.add_edge(read.id, turn, EdgeKind::CausedBy)?;
    if read.outcome != StepOutcome::Refused {
        work.charge(BudgetKind::EvidenceRequests, 1);
    }
    Ok(())
}

/// Why a question stopped waiting.
#[derive(Debug, Clone, Copy)]
pub(super) enum Resume {
    /// A person answered.
    Answer,
    /// Nobody answered in time.
    Timeout,
}

impl Resume {
    const fn name(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Timeout => "timeout",
        }
    }
}

/// Asks for the next turn of an investigation whose newest turn put `question`: one
/// `BuildContext` effect. A later turn that already exists means the question is old news.
pub(super) fn resume(work: &mut Work<'_>, question: StepId, how: Resume) {
    if work.op().kind != OperationKind::Investigate {
        return;
    }
    let Some(newest) = work
        .snap
        .steps
        .iter()
        .filter(|record| record.step.kind == StepKind::ReasoningTurn)
        .max_by_key(|record| record.step.ordinal)
    else {
        return;
    };
    let turn = newest.step.id;
    let asked =
        work.snap.edges.iter().any(|edge| {
            edge.kind == EdgeKind::CausedBy && edge.from == question && edge.to == turn
        });
    if !asked || !newest.progress.outcome.is_final() {
        return;
    }
    let (operation, cycle, now) = (work.op().id, work.op().cycle, work.now);
    work.effect(
        EffectKind::BuildContext,
        resume_payload(operation, cycle, how.name(), question),
        now,
        BUILD_CONTEXT_ATTEMPTS,
        resume_key(operation, cycle, question),
    );
}
