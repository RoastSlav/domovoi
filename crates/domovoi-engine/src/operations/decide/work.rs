//! The scratch copy a decision is built on: it applies each change to a clone of the
//! snapshot as it records it, so later rules in the same event see earlier ones.

use domovoi_core::operation::{MAX_OUTPUT_BYTES, MAX_SUMMARY_CHARS, cut_chars};
use domovoi_core::ports::{EffectKind, JournalKind, Versioned};
use domovoi_core::{
    BudgetKind, EdgeKind, ExecutionId, ExecutionRecord, ExecutionState, Operation,
    OperationSnapshot, OperationStatus, PauseReason, Step, StepEdge, StepId, StepIntent,
    StepOutcome, StepProgress, StepRecord, check_edge,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

use super::effects::{WAKE_ATTEMPTS, wake_payload, wall_key};
use super::status::derive_status;
use super::{AuditNote, DecideError, Decision, EffectSpec, IdSource, JournalNote, ProgressChange};
use crate::operations::audit;
use crate::operations::journal;
use crate::operations::state::OperationState;

/// `time` moved on by `by`, saturating at the end of the time line.
pub(super) fn after(time: Timestamp, by: SignedDuration) -> Timestamp {
    let limit = if by.is_negative() {
        Timestamp::MIN
    } else {
        Timestamp::MAX
    };
    time.checked_add(by).unwrap_or(limit)
}

/// `seconds` as a duration, saturating.
pub(super) fn secs(seconds: u64) -> SignedDuration {
    SignedDuration::from_secs(i64::try_from(seconds).unwrap_or(i64::MAX))
}

/// `text` cut to at most [`MAX_SUMMARY_CHARS`] characters.
pub(super) fn short_text(text: &str) -> String {
    cut_chars(text, MAX_SUMMARY_CHARS)
}

/// `text` cut to at most [`MAX_OUTPUT_BYTES`] bytes on a character boundary.
pub(super) fn cut_output(text: &str) -> String {
    let mut end = text.len().min(MAX_OUTPUT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// The working state of one call to `decide`.
pub(super) struct Work<'a> {
    pub(super) now: Timestamp,
    pub(super) snap: OperationSnapshot,
    ids: &'a mut dyn IdSource,
    before: (OperationStatus, u32),
    steps: Vec<StepRecord>,
    edges: Vec<StepEdge>,
    executions: Vec<ExecutionRecord>,
    progress: Vec<ProgressChange>,
    effects: Vec<EffectSpec>,
    journal: Vec<JournalNote>,
    audit: Vec<AuditNote>,
    pub(super) summary: Option<String>,
}

impl<'a> Work<'a> {
    pub(super) fn new(state: &OperationState, now: Timestamp, ids: &'a mut dyn IdSource) -> Self {
        let snap = state.snapshot().clone();
        let before = (snap.operation.status.clone(), snap.operation.cycle);
        Self {
            now,
            snap,
            ids,
            before,
            steps: Vec::new(),
            edges: Vec::new(),
            executions: Vec::new(),
            progress: Vec::new(),
            effects: Vec::new(),
            journal: Vec::new(),
            audit: Vec::new(),
            summary: None,
        }
    }

    pub(super) fn op(&self) -> &Operation {
        &self.snap.operation
    }

    pub(super) fn op_mut(&mut self) -> &mut Operation {
        &mut self.snap.operation
    }

    pub(super) fn find(&self, id: StepId) -> Result<&StepRecord, DecideError> {
        self.snap.step(id).ok_or(DecideError::UnknownStep(id))
    }

    pub(super) fn new_step_id(&mut self) -> StepId {
        self.ids.step()
    }

    /// The Approval step of Execution step `execution`.
    pub(super) fn approval_of(&self, execution: StepId) -> Option<&StepRecord> {
        self.snap.steps.iter().find(|record| {
            matches!(&record.step.intent, StepIntent::Approval { execution: e, .. } if *e == execution)
        })
    }

    /// The derived state of Execution step `step`, when it has an execution row.
    pub(super) fn exec_state(&self, step: StepId) -> Option<ExecutionState> {
        let row = self.snap.execution_of(step)?;
        let outcome = self.snap.step(step)?.progress.outcome;
        let approval = self.approval_of(step).map(|r| r.progress.outcome);
        Some(row.value.state(outcome, approval))
    }

    /// Adds a step with its born progress, checking it first.
    pub(super) fn add_step(
        &mut self,
        id: StepId,
        phase: &str,
        parent: Option<StepId>,
        intent: StepIntent,
        born: StepProgress,
    ) -> Result<(), DecideError> {
        if self.snap.step(id).is_some() {
            return Err(DecideError::Invalid(
                "a step with that id already exists".into(),
            ));
        }
        if let Some(parent) = parent {
            self.find(parent)?;
        }
        let step = Step {
            id,
            operation: self.op().id,
            ordinal: u32::try_from(self.snap.steps.len()).unwrap_or(u32::MAX),
            kind: intent.kind(),
            phase: phase.to_owned(),
            parent,
            cycle: self.op().cycle,
            intent,
            created_at: self.now,
        };
        step.validate()
            .and_then(|()| born.validate())
            .map_err(|e| DecideError::Invalid(e.to_string()))?;
        self.note(JournalKind::StepCreated, journal::step_created(&step));
        self.announce(id, StepOutcome::Pending, &born);
        let record = StepRecord {
            step,
            progress: born,
        };
        self.snap.steps.push(record.clone());
        self.steps.push(record);
        Ok(())
    }

    /// Adds an edge after the edge rules.
    pub(super) fn add_edge(
        &mut self,
        from: StepId,
        to: StepId,
        kind: EdgeKind,
    ) -> Result<(), DecideError> {
        let (Some(f), Some(t)) = (self.snap.step(from), self.snap.step(to)) else {
            return Err(DecideError::BadEdge(
                "an edge must join steps of this operation".into(),
            ));
        };
        check_edge(&f.step, &t.step, kind).map_err(|e| DecideError::BadEdge(e.to_string()))?;
        let edge = StepEdge {
            operation: self.op().id,
            from,
            to,
            kind,
        };
        if self.snap.edges.contains(&edge) {
            return Err(DecideError::BadEdge("that edge already exists".into()));
        }
        self.snap.edges.push(edge);
        self.edges.push(edge);
        Ok(())
    }

    /// Adds the execution row of a newly proposed Execution step.
    pub(super) fn add_execution(&mut self, record: ExecutionRecord) {
        self.snap.executions.push(Versioned {
            value: record.clone(),
            version: 0,
        });
        self.executions.push(record);
    }

    /// The progress `previous` becomes when it ends as `outcome`.
    pub(super) fn ended(
        &self,
        previous: &StepProgress,
        outcome: StepOutcome,
        summary: Option<&str>,
        reason: Option<&str>,
        output: Option<&str>,
    ) -> StepProgress {
        StepProgress {
            outcome,
            started_at: previous.started_at,
            ended_at: Some(self.now),
            summary: summary.map(short_text),
            reason: reason.map(short_text),
            output: output.map(cut_output),
        }
    }

    /// Changes an existing step's progress and journals the change.
    pub(super) fn set_progress(&mut self, id: StepId, to: StepProgress) -> Result<(), DecideError> {
        to.validate()
            .map_err(|e| DecideError::Invalid(e.to_string()))?;
        let from = self.find(id)?.progress.outcome;
        self.announce(id, from, &to);
        self.progress.push(ProgressChange {
            step: id,
            from,
            to: to.clone(),
        });
        if let Some(record) = self.snap.steps.iter_mut().find(|r| r.step.id == id) {
            record.progress = to;
        }
        Ok(())
    }

    /// Journals a step starting or finishing.
    fn announce(&mut self, id: StepId, from: StepOutcome, to: &StepProgress) {
        if to.outcome == StepOutcome::Running && from != StepOutcome::Running {
            self.note(JournalKind::StepStarted, journal::step_started(id));
        } else if to.outcome.is_final() {
            self.note(JournalKind::StepFinished, journal::step_finished(id, to));
        }
    }

    pub(super) fn note(&mut self, kind: JournalKind, payload: Value) {
        self.journal.push(JournalNote { kind, payload });
    }

    /// Journals an execution moving between states.
    pub(super) fn transition(
        &mut self,
        step: StepId,
        from: ExecutionState,
        to: ExecutionState,
        reason: Option<&str>,
    ) {
        if from != to {
            let payload = journal::execution_transition(step, from, to, reason);
            self.note(JournalKind::ExecutionTransition, payload);
        }
    }

    pub(super) fn effect(
        &mut self,
        kind: EffectKind,
        payload: Value,
        not_before: Timestamp,
        max_attempts: u32,
        dedup_key: String,
    ) {
        let id = self.ids.effect();
        self.effects.push(EffectSpec {
            id,
            kind,
            payload,
            not_before,
            max_attempts,
            dedup_key: Some(dedup_key),
        });
    }

    /// Records an audit entry about `step`'s execution; `extra` fields join the detail.
    pub(super) fn audit_execution(&mut self, kind: &'static str, step: StepId, extra: Value) {
        let subject = format!("execution:{}", ExecutionId::from_step(step));
        self.audit(kind, subject, Some(step), extra);
    }

    pub(super) fn audit(
        &mut self,
        kind: &'static str,
        subject: String,
        step: Option<StepId>,
        extra: Value,
    ) {
        let mut detail = json!({
            "operation": self.op().id.to_string(),
            "step": step.map(|s| s.to_string()),
        });
        if let (Some(target), Value::Object(fields)) = (detail.as_object_mut(), extra) {
            target.extend(fields);
        }
        self.audit.push(AuditNote {
            kind,
            subject,
            detail,
        });
    }

    /// Records an audit entry about the operation itself.
    pub(super) fn audit_operation(
        &mut self,
        kind: &'static str,
        step: Option<StepId>,
        extra: Value,
    ) {
        let subject = format!("operation:{}", self.op().id);
        self.audit(kind, subject, step, extra);
    }

    /// Stops the operation for `reason`, unless it is already stopped or over.
    pub(super) fn pause(&mut self, reason: PauseReason) {
        let status = &self.op().status;
        if status.is_terminal() || matches!(status, OperationStatus::Paused { .. }) {
            return;
        }
        let step = match &reason {
            PauseReason::OutcomeUnknown { step } => Some(*step),
            _ => None,
        };
        let detail = json!({ "reason": serde_json::to_value(&reason).unwrap_or(Value::Null) });
        self.audit_operation(audit::OPERATION_PAUSED, step, detail);
        self.op_mut().status = OperationStatus::Paused { reason };
    }

    /// Charges `amount` to a budget.
    pub(super) fn charge(&mut self, kind: BudgetKind, amount: u64) {
        self.op_mut().budgets.charge(kind, amount);
    }

    /// Schedules the wake that measures wall time `limit_secs` after `started`.
    pub(super) fn wall_wake(&mut self, started: Timestamp, limit_secs: u64) {
        let op = self.op().id;
        let due = after(started, secs(limit_secs));
        let payload = wake_payload(op, None, "wall_time");
        let key = wall_key(op, limit_secs);
        self.effect(EffectKind::Wake, payload, due, WAKE_ATTEMPTS, key);
    }

    /// Settles the status of an open operation from its steps, closes the books and returns
    /// the decision.
    pub(super) fn into_decision(mut self) -> Decision {
        let derived = derive_status(&self.snap);
        let now = self.now;
        let op = self.op_mut();
        if matches!(
            op.status,
            OperationStatus::Running
                | OperationStatus::WaitingApproval
                | OperationStatus::WaitingUser
                | OperationStatus::WaitingAgent
                | OperationStatus::WaitingTimer
        ) {
            op.status = derived;
        }
        if op.status.is_terminal() && op.ended_at.is_none() {
            op.ended_at = Some(now);
        }
        op.updated_at = now;
        let operation = op.clone();
        if operation.status != self.before.0 || operation.cycle != self.before.1 {
            let payload = journal::operation_status(
                &self.before.0,
                &operation.status,
                operation.cycle,
                self.summary.as_deref(),
            );
            self.note(JournalKind::OperationStatus, payload);
        }
        Decision {
            operation,
            steps: self.steps,
            edges: self.edges,
            executions: self.executions,
            progress: self.progress,
            effects: self.effects,
            journal: self.journal,
            audit: self.audit,
        }
    }
}
