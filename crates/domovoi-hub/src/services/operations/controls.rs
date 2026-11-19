//! Cancel, pause, resume and answer: the controls a person has over a running operation.
//!
//! A precheck reads the stored state to give an accurate refusal and to make a repeat of
//! cancel or pause a no-op; the operation writer re-checks everything, so a lost race still
//! ends in a refusal from the writer.

use domovoi_core::{OperationId, OperationStatus, StepId, StepKind, StepOutcome};

use super::limits::{ANSWER_MAX, CANCEL_REASON_MAX, DEFAULT_CANCEL_REASON, checked_extensions};
use super::{BudgetExtension, OperationCommandError, OperationCommands};
use crate::services::Caller;

const FINISHED: &str = "the operation has already finished";

fn conflict(text: &str) -> OperationCommandError {
    OperationCommandError::Conflict(text.to_owned())
}

fn store_error(e: domovoi_core::ports::StoreError) -> OperationCommandError {
    OperationCommandError::Operation(domovoi_engine::operations::OperationError::Store(
        e.to_string(),
    ))
}

/// `text` cut to at most `max` characters, at a character boundary.
fn cut(text: String, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => text[..at].to_owned(),
        None => text,
    }
}

impl OperationCommands {
    async fn status(&self, op: OperationId) -> Result<OperationStatus, OperationCommandError> {
        self.repo
            .get(op)
            .await
            .map_err(store_error)?
            .map(|stored| stored.value.status)
            .ok_or(OperationCommandError::NotFound)
    }

    /// Cancels `op`. Pending steps and effects are cancelled; a run already dispatched is not
    /// recalled and its result is still recorded. Cancelling a cancelled operation changes
    /// nothing.
    pub async fn cancel(
        &self,
        caller: &Caller,
        op: OperationId,
        reason: Option<&str>,
    ) -> Result<(), OperationCommandError> {
        let handle = self.handle()?;
        let reason = reason.map(str::trim).filter(|text| !text.is_empty());
        let reason = reason.unwrap_or(DEFAULT_CANCEL_REASON);
        if reason.chars().count() > CANCEL_REASON_MAX {
            return Err(OperationCommandError::Invalid(
                "the reason must have at most 500 characters".to_owned(),
            ));
        }
        let reason = self.redactor.redact(reason);
        match self.status(op).await? {
            OperationStatus::Cancelled => Ok(()),
            status if status.is_terminal() => Err(conflict(FINISHED)),
            _ => handle
                .cancel(op, caller.actor(), reason)
                .await
                .map_err(OperationCommandError::Operation),
        }
    }

    /// Pauses `op` until a person resumes it. Pausing a paused operation changes nothing.
    pub async fn pause(
        &self,
        caller: &Caller,
        op: OperationId,
    ) -> Result<(), OperationCommandError> {
        let handle = self.handle()?;
        match self.status(op).await? {
            OperationStatus::Paused { .. } => Ok(()),
            OperationStatus::Queued => Err(conflict(
                "the operation has not started yet; cancel it instead",
            )),
            status if status.is_terminal() => Err(conflict(FINISHED)),
            _ => handle
                .pause(op, caller.actor())
                .await
                .map_err(OperationCommandError::Operation),
        }
    }

    /// Resumes a paused `op`, raising the named limits first. The limits are always numbers:
    /// a person cannot make a budget unlimited from here.
    pub async fn resume(
        &self,
        caller: &Caller,
        op: OperationId,
        extend: &[BudgetExtension],
    ) -> Result<(), OperationCommandError> {
        let handle = self.handle()?;
        let extend = checked_extensions(extend)?;
        if !matches!(self.status(op).await?, OperationStatus::Paused { .. }) {
            return Err(conflict("the operation is not paused"));
        }
        handle
            .resume(op, caller.actor(), extend)
            .await
            .map_err(OperationCommandError::Operation)
    }

    /// Answers the open question `question` of `op`. The text is trimmed, bounded and
    /// redacted before it is stored; it is data for the next model turn, never an instruction
    /// to the hub.
    pub async fn answer(
        &self,
        caller: &Caller,
        op: OperationId,
        question: StepId,
        text: &str,
    ) -> Result<(), OperationCommandError> {
        let handle = self.handle()?;
        let text = text.trim();
        if text.is_empty() || text.chars().count() > ANSWER_MAX {
            return Err(OperationCommandError::Invalid(
                "the answer must have 1 to 2000 characters".to_owned(),
            ));
        }
        let snapshot = self
            .repo
            .load(op)
            .await
            .map_err(store_error)?
            .ok_or(OperationCommandError::NotFound)?;
        let step = snapshot
            .step(question)
            .ok_or(OperationCommandError::NotFound)?;
        if step.step.kind != StepKind::Question {
            return Err(conflict("that step is not a question"));
        }
        if step.progress.outcome != StepOutcome::Pending {
            return Err(conflict("this question is not waiting for an answer"));
        }
        let status = &snapshot.operation.status;
        if status.is_terminal() {
            return Err(conflict(FINISHED));
        }
        if matches!(status, OperationStatus::Paused { .. }) {
            return Err(conflict(
                "the operation is paused; resume it before answering",
            ));
        }
        let text = cut(self.redactor.redact(text), ANSWER_MAX);
        handle
            .answer(op, question, text, caller.actor())
            .await
            .map(|_| ())
            .map_err(OperationCommandError::Operation)
    }
}
