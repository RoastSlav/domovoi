//! The coarse status: what an open operation shows, and which events each status accepts.

use domovoi_core::{Operation, OperationSnapshot, OperationStatus, WaitKind};

use super::DecideError;

/// The status an open operation shows, from its steps alone: waiting for an approval, else a
/// person's answer, else an agent, else a timer, else running. The first match wins.
///
/// Applies while the operation is `Running` or `Waiting*`; the other statuses are never
/// derived.
#[must_use]
pub fn derive_status(snapshot: &OperationSnapshot) -> OperationStatus {
    match snapshot.waits().first() {
        Some(WaitKind::Approval) => OperationStatus::WaitingApproval,
        Some(WaitKind::User) => OperationStatus::WaitingUser,
        Some(WaitKind::Agent) => OperationStatus::WaitingAgent,
        Some(WaitKind::Timer) => OperationStatus::WaitingTimer,
        None => OperationStatus::Running,
    }
}

/// The statuses an event is accepted in. Anything else is refused: a finished operation or
/// one in a status the event does not apply to is `OperationClosed`, a paused one `Paused`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Allow {
    queued: bool,
    active: bool,
    paused: bool,
}

impl Allow {
    /// Only a queued operation.
    pub(super) const QUEUED_ONLY: Self = Self::of(true, false, false);
    /// Only a running or waiting operation.
    pub(super) const ACTIVE: Self = Self::of(false, true, false);
    /// Queued, running or waiting.
    pub(super) const QUEUED_ACTIVE: Self = Self::of(true, true, false);
    /// Running, waiting or paused.
    pub(super) const ACTIVE_PAUSED: Self = Self::of(false, true, true);
    /// Anything not finished.
    pub(super) const OPEN: Self = Self::of(true, true, true);
    /// Only a paused operation.
    pub(super) const PAUSED_ONLY: Self = Self::of(false, false, true);

    const fn of(queued: bool, active: bool, paused: bool) -> Self {
        Self {
            queued,
            active,
            paused,
        }
    }
}

/// Refuses the event unless the operation's status is one of `allow`.
pub(super) fn gate(operation: &Operation, allow: Allow) -> Result<(), DecideError> {
    let status = &operation.status;
    if status.is_terminal() {
        return Err(DecideError::OperationClosed);
    }
    match status {
        OperationStatus::Paused { .. } if allow.paused => Ok(()),
        OperationStatus::Paused { .. } => Err(DecideError::Paused),
        OperationStatus::Queued if allow.queued => Ok(()),
        OperationStatus::Queued => Err(DecideError::OperationClosed),
        _ if allow.active => Ok(()),
        _ => Err(DecideError::OperationClosed),
    }
}

/// The gate for an event that records how a step ended. A finished operation still accepts
/// the end of a step that was running, and only records it (the result is `true`); a paused
/// one accepts it when `paused_needs_running` is false or the step was running.
pub(super) fn gate_settle(
    operation: &Operation,
    step_running: bool,
    paused_needs_running: bool,
) -> Result<bool, DecideError> {
    let status = &operation.status;
    if status.is_terminal() {
        return if step_running {
            Ok(true)
        } else {
            Err(DecideError::OperationClosed)
        };
    }
    match status {
        OperationStatus::Paused { .. } if paused_needs_running && !step_running => {
            Err(DecideError::Paused)
        }
        OperationStatus::Queued => Err(DecideError::OperationClosed),
        _ => Ok(false),
    }
}
