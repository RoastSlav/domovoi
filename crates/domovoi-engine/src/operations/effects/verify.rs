//! Verifying a finished execution: re-run the check it was about and record whether the
//! condition is gone.
//!
//! The re-check only reads, so running it twice is harmless; that is why it is retried freely.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectRecord, LinkError};
use domovoi_core::{SignalStatus, StepKind, StepOutcome};

use super::{Address, address, after, ask, finish, held, load, outcome, unreadable};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::writer::{OpShared, OperationCommand};

/// Re-runs the pinned check after a successful execution and hands the answer to the writer.
pub(super) struct VerifyHandler {
    shared: Arc<OpShared>,
}

impl VerifyHandler {
    /// A handler over the operation service's writer and ports.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl EffectHandler for VerifyHandler {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &*self.shared;
        let held = held(effect, claim);
        let (
            Some(Address {
                operation,
                step: Some(step),
            }),
            Some(execution),
        ) = (
            address(&effect.payload),
            effect
                .payload
                .get("execution")
                .and_then(|id| id.as_str())
                .and_then(|id| id.parse().ok()),
        )
        else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        let open = snapshot.step(step).is_some_and(|record| {
            record.step.kind == StepKind::Verification
                && matches!(
                    record.progress.outcome,
                    StepOutcome::Pending | StepOutcome::Running
                )
        });
        let Some(row) = snapshot
            .execution_of(execution)
            .map(|stored| stored.value.clone())
            .filter(|_| open)
        else {
            return finish(shared, held).await;
        };
        let deadline = after(shared, shared.settings.recheck_deadline);
        let recheck = shared
            .deps
            .rerun
            .rerun_check(row.host, row.check.clone(), row.target.clone(), deadline)
            .await;
        let redactor = &shared.deps.redactor;
        let (passed, summary) = match recheck {
            Ok(result) => (
                result.status == SignalStatus::Resolved,
                redactor.redact(&result.summary),
            ),
            Err(err) => {
                let transient = matches!(
                    err,
                    LinkError::NotConnected
                        | LinkError::Timeout
                        | LinkError::Transport(_)
                        | LinkError::Indeterminate(_)
                );
                if transient && effect.attempt < effect.max_attempts {
                    return EffectOutcome::Retry {
                        error: redactor.redact(&err.to_string()),
                    };
                }
                (
                    false,
                    redactor.redact(&format!("the re-check could not run: {err}")),
                )
            }
        };
        let answered = ask(shared, |reply| OperationCommand::Verified {
            held,
            operation,
            step,
            passed,
            summary,
            reply,
        })
        .await;
        outcome(answered)
    }
}
