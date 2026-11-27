//! Telling the rest of the hub that a step is no longer waiting for approval.

use super::OpShared;
use super::commit::Applied;
use crate::events::{DomainEvent, EventKind};

/// Publishes `ExecutionChanged` for each execution that left "awaiting approval" in the
/// commit that produced `applied`. Nothing here can fail the commit.
pub(super) async fn left_approval(shared: &OpShared, applied: &Applied) {
    let Some(incident) = applied.incident else {
        return;
    };
    if applied.left_approval.is_empty() && applied.questions.is_empty() {
        return;
    }
    let incident = match shared.deps.incidents.get(incident).await {
        Ok(Some(incident)) => incident,
        Ok(None) => {
            tracing::debug!(%incident, "an execution changed for an incident that is gone");
            return;
        }
        Err(err) => {
            tracing::debug!(%err, "could not load the incident of a changed execution");
            return;
        }
    };
    for text in &applied.questions {
        let kind = EventKind::QuestionAsked {
            text: shared.deps.redactor.redact(text),
        };
        if shared
            .deps
            .bus
            .send(DomainEvent::new(kind, &incident, true))
            .is_err()
        {
            tracing::debug!("no one listens for questions");
        }
    }
    for execution in &applied.left_approval {
        let event = DomainEvent::new(
            EventKind::ExecutionChanged {
                execution: *execution,
            },
            &incident,
            true,
        );
        if shared.deps.bus.send(event).is_err() {
            tracing::debug!(%execution, "no one listens for execution changes");
        }
    }
}

/// The text of each question step a decision adds.
pub(super) fn questions(decision: &crate::operations::decide::Decision) -> Vec<String> {
    decision
        .steps
        .iter()
        .filter_map(|step| match &step.step.intent {
            domovoi_core::StepIntent::Question { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}
