//! The changes the writer builds for people's decisions and for executions ending.
//!
//! Text that came from outside (a reason, a summary, an output) is redacted again here, so
//! nothing reaches the store or the journal unredacted whichever caller built it.

use domovoi_core::{
    Actor, ApprovalProvenance, Authorization, HumanApproval, JournalEpoch, OperationId,
    PlanBinding, SYSTEM_OPERATIONS, StepId, StepOutcome,
};
use jiff::SignedDuration;

use super::OpShared;
use super::commands::Held;
use super::commit::{AuthorizeEdit, Change, Edit};
use crate::operations::decide::OperationEvent;

/// The actor for what the service does on its own.
pub(super) fn system() -> Actor {
    Actor::System(SYSTEM_OPERATIONS.to_owned())
}

/// A person approved: the row takes the approval first, then the decision sees it.
pub(super) fn authorize(
    operation: OperationId,
    step: StepId,
    authorization: Authorization,
    binding: PlanBinding,
    provenance: ApprovalProvenance,
) -> Change {
    let actor = authorization
        .human_approval()
        .map_or_else(system, |approval| Actor::User(approval.user()));
    let mut change = Change::new(
        operation,
        actor,
        OperationEvent::ExecutionAuthorized { step },
    );
    change.edit = Edit::Authorize(Box::new(AuthorizeEdit {
        step,
        authorization,
        binding,
        provenance,
    }));
    change
}

/// A person denied the execution.
pub(super) fn deny(operation: OperationId, step: StepId, approval: &HumanApproval) -> Change {
    Change::new(
        operation,
        Actor::User(approval.user()),
        OperationEvent::ExecutionDenied { step },
    )
}

/// An execution ended. The row records when; a success may owe a verification.
pub(super) struct Settled {
    pub(super) held: Option<Held>,
    pub(super) operation: OperationId,
    pub(super) step: StepId,
    pub(super) outcome: StepOutcome,
    pub(super) reason: Option<String>,
    pub(super) output: Option<String>,
    pub(super) verify_after: Option<SignedDuration>,
}

/// The change that records an execution's end.
pub(super) fn settle(shared: &OpShared, settled: Settled) -> Change {
    let redact = |text: String| shared.deps.redactor.redact(&text);
    let mut change = Change::new(
        settled.operation,
        system(),
        OperationEvent::ExecutionSettled {
            step: settled.step,
            outcome: settled.outcome,
            reason: settled.reason.map(redact),
            output: settled.output.map(redact),
            verify_after: settled.verify_after,
        },
    );
    change.edit = Edit::Settled(settled.step);
    change.held = settled.held;
    change
}

/// The change that records the hand-off of an execution to its agent.
pub(super) fn dispatched(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    step: StepId,
    epoch: JournalEpoch,
) -> Change {
    let reconcile_after = shared
        .deps
        .clock
        .now()
        .checked_add(shared.settings.reconcile_delay)
        .unwrap_or(jiff::Timestamp::MAX);
    let mut change = Change::new(
        operation,
        system(),
        OperationEvent::ExecutionDispatched {
            step,
            epoch,
            reconcile_after,
        },
    );
    change.edit = Edit::Dispatched(step, epoch);
    change.held = Some(held);
    change
}
