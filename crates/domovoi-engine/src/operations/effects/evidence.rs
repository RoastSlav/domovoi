//! Collecting evidence: one read tool on one host, for one EvidenceRequest step.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord};
use domovoi_core::{OperationId, OperationStatus, ReadToolCall, StepIntent, StepKind, StepOutcome};

use super::{Address, address, ask, held, load, outcome, unreadable};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::evidence::{EvidenceError, EvidenceRequest};
use crate::operations::error::OperationError;
use crate::operations::writer::{Held, OpShared, OperationCommand};

/// How many characters of the output the step's one-line summary keeps.
const SUMMARY_CHARS: usize = 280;

/// Runs the read tool an EvidenceRequest step names and records what came back.
pub(super) struct CollectEvidence {
    shared: Arc<OpShared>,
}

impl CollectEvidence {
    /// A handler over the operation service's writer and ports.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl EffectHandler for CollectEvidence {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &*self.shared;
        let held = held(effect, claim);
        let Some(Address {
            operation,
            step: Some(step),
        }) = address(effect)
        else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        let Some(record) = snapshot.step(step) else {
            return super::finish(shared, held).await;
        };
        let open = !snapshot.operation.status.is_terminal() && !record.progress.outcome.is_final();
        if !open || record.step.kind != StepKind::EvidenceRequest {
            return super::finish(shared, held).await;
        }
        let StepIntent::EvidenceRequest { host, tool, args } = &record.step.intent else {
            return unreadable();
        };
        if snapshot.operation.status.pause_reason().is_some() {
            return wait(shared, held, effect, operation).await;
        }
        if record.progress.outcome == StepOutcome::Pending {
            let started = ask(shared, |reply| OperationCommand::StartStep {
                held,
                operation,
                step,
                reply,
            })
            .await;
            if let Err(err) = started {
                tracing::debug!(%err, %step, "an evidence step could not be started");
                // A queued operation has no slot yet: the read waits and is never dropped. The
                // writer decides this against the stored operation, so a snapshot that was
                // read just before admission cannot send a read away for nothing.
                if matches!(err, OperationError::Decide(_)) && queued(shared, operation).await {
                    return wait(shared, held, effect, operation).await;
                }
                return match outcome(Err(err)) {
                    EffectOutcome::Handled => super::finish(shared, held).await,
                    other => other,
                };
            }
        }
        let call = match ReadToolCall::from_json(tool, args.clone()) {
            Ok(call) => call,
            Err(err) => {
                return end(
                    shared,
                    held,
                    operation,
                    step,
                    Ending::Failed(err.to_string()),
                )
                .await;
            }
        };
        let deadline = super::after(shared, shared.settings.evidence_deadline);
        let request = EvidenceRequest::for_host(*host, call, deadline);
        match shared.deps.evidence.collect_evidence(request).await {
            Ok(output) => {
                let text = shared.deps.redactor.redact(&output.text);
                let summary: String = text.chars().take(SUMMARY_CHARS).collect();
                let ending = Ending::Succeeded { summary, text };
                end(shared, held, operation, step, ending).await
            }
            Err(
                err @ (EvidenceError::NotConnected
                | EvidenceError::Incompatible
                | EvidenceError::NotEnabled(_)),
            ) => {
                let reason = shared.deps.redactor.redact(&err.to_string());
                end(shared, held, operation, step, Ending::Unavailable(reason)).await
            }
            Err(err) => {
                let reason = shared.deps.redactor.redact(&err.to_string());
                if effect.attempt < effect.max_attempts {
                    return EffectOutcome::Retry { error: reason };
                }
                end(shared, held, operation, step, Ending::Failed(reason)).await
            }
        }
    }
}

/// Whether the stored operation is still waiting for a slot.
async fn queued(shared: &OpShared, operation: OperationId) -> bool {
    matches!(
        load(shared, operation).await,
        Ok(snapshot) if snapshot.operation.status == OperationStatus::Queued
    )
}

/// Puts the read back to be looked at again after the retry interval.
async fn wait(
    shared: &OpShared,
    held: Held,
    effect: &EffectRecord,
    operation: OperationId,
) -> EffectOutcome {
    let again = super::spec(
        EffectKind::CollectEvidence,
        effect.payload.clone(),
        super::after(shared, shared.settings.retry_interval),
        effect.max_attempts,
        effect.dedup_key.clone(),
    );
    super::defer(shared, held, operation, None, again, false).await
}

/// How a read ended.
enum Ending {
    Succeeded { summary: String, text: String },
    Unavailable(String),
    Failed(String),
}

/// Records how the read ended and completes the effect.
async fn end(
    shared: &OpShared,
    held: crate::operations::writer::Held,
    operation: domovoi_core::OperationId,
    step: domovoi_core::StepId,
    ending: Ending,
) -> EffectOutcome {
    let (result, summary, reason, output) = match ending {
        Ending::Succeeded { summary, text } => {
            (StepOutcome::Succeeded, Some(summary), None, Some(text))
        }
        Ending::Unavailable(reason) => (StepOutcome::Unavailable, None, Some(reason), None),
        Ending::Failed(reason) => (StepOutcome::Failed, None, Some(reason), None),
    };
    let answered = ask(shared, |reply| OperationCommand::FinishStep {
        held,
        operation,
        step,
        outcome: result,
        summary,
        reason,
        output,
        reply,
    })
    .await;
    outcome(answered)
}
