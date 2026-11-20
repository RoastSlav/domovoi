//! The `draft_resolution` effect: writes the summary of a finished operation.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectRecord};

use super::narrative::draft_narrative;
use super::plan::plan_draft;
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::effects::{address, unreadable};
use crate::operations::effects::{ask, held, outcome};
use crate::operations::writer::{OpShared, OperationCommand};

/// Drafts the summary of the operation an effect names.
pub(in crate::operations) struct DraftResolutionHandler {
    shared: Arc<OpShared>,
}

impl DraftResolutionHandler {
    /// A handler over the operation service's writer and ports.
    pub(in crate::operations) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl EffectHandler for DraftResolutionHandler {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &*self.shared;
        let held = held(effect, claim);
        let Some(address) = address(effect) else {
            return unreadable();
        };
        let operation = address.operation;
        let snapshot = match shared.deps.operations.load(operation).await {
            Ok(Some(snapshot)) if snapshot.operation.status.is_terminal() => snapshot,
            Ok(_) => return finish(shared, held).await,
            Err(err) => {
                return EffectOutcome::Retry {
                    error: err.to_string(),
                };
            }
        };
        let existing = match shared.deps.experience.current(operation).await {
            Ok(existing) => existing,
            Err(err) => {
                return EffectOutcome::Retry {
                    error: err.to_string(),
                };
            }
        };
        let narrative = draft_narrative(&snapshot);
        let revisions = plan_draft(
            &snapshot,
            &existing,
            narrative.as_ref(),
            shared.deps.redactor.as_ref(),
        );
        if revisions.is_empty() {
            return finish(shared, held).await;
        }
        outcome(
            ask(shared, |reply| OperationCommand::WriteResolution {
                operation,
                revisions,
                held,
                reply,
            })
            .await,
        )
    }
}

async fn finish(shared: &OpShared, held: crate::operations::writer::Held) -> EffectOutcome {
    crate::operations::effects::finish(shared, held).await
}
