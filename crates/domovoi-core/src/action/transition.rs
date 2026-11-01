//! The action state machine. Every method checks first and mutates only on success.

use jiff::Timestamp;

use super::approval::{ApprovalChannel, HumanApproval, PlanHash};
use super::request::{ActionRequest, ActionStatus};
use crate::skill::Risk;

/// A status change, for the caller to persist and audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionChange {
    /// Status before.
    pub from: ActionStatus,
    /// Status after.
    pub to: ActionStatus,
}

/// Why a transition was refused. The request is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionError {
    /// The operation is not allowed in the current status.
    #[error("cannot {operation} an action that is {status:?}")]
    NotAllowed {
        /// Status the request was in.
        status: ActionStatus,
        /// The refused operation.
        operation: &'static str,
    },
    /// The approval window has closed.
    #[error("the approval window has closed")]
    Expired,
    /// The approval window is still open.
    #[error("the approval window is still open")]
    NotYetExpired,
    /// Destructive actions need a typed confirmation in the web UI (INV-6).
    #[error("destructive actions need a typed confirmation in the web UI")]
    DestructiveNeedsTypedConfirmation,
}

impl ActionRequest {
    fn require(&self, ok: bool, operation: &'static str) -> Result<(), ActionError> {
        if ok {
            Ok(())
        } else {
            Err(ActionError::NotAllowed {
                status: self.status.clone(),
                operation,
            })
        }
    }

    fn move_to(&mut self, to: ActionStatus, now: Timestamp) -> ActionChange {
        let from = std::mem::replace(&mut self.status, to.clone());
        self.updated_at = now;
        ActionChange { from, to }
    }

    /// `Proposed → Approved`. Requires an open window and, for destructive skills, a typed
    /// confirmation in the web UI.
    pub fn approve(
        &mut self,
        approval: HumanApproval,
        now: Timestamp,
    ) -> Result<ActionChange, ActionError> {
        self.require(self.status == ActionStatus::Proposed, "approve")?;
        if now >= self.expires_at {
            return Err(ActionError::Expired);
        }
        let typed_on_web = matches!(
            approval.channel(),
            ApprovalChannel::Web {
                typed_confirmation: true
            }
        );
        if self.risk == Risk::Destructive && !typed_on_web {
            return Err(ActionError::DestructiveNeedsTypedConfirmation);
        }
        self.approved_by = Some(approval);
        Ok(self.move_to(ActionStatus::Approved, now))
    }

    /// `Proposed → Denied`.
    pub fn deny(&mut self, now: Timestamp) -> Result<ActionChange, ActionError> {
        self.require(self.status == ActionStatus::Proposed, "deny")?;
        Ok(self.move_to(ActionStatus::Denied, now))
    }

    /// `Proposed → Expired`, once the approval window has closed.
    pub fn expire(&mut self, now: Timestamp) -> Result<ActionChange, ActionError> {
        self.require(self.status == ActionStatus::Proposed, "expire")?;
        if now < self.expires_at {
            return Err(ActionError::NotYetExpired);
        }
        Ok(self.move_to(ActionStatus::Expired, now))
    }

    /// `Approved → Dispatched` if `current_plan_hash` (recomputed from the current manifest
    /// and arguments) matches the approved plan, otherwise `Approved → Rejected` (INV-4).
    pub fn dispatch(
        &mut self,
        current_plan_hash: PlanHash,
        now: Timestamp,
    ) -> Result<ActionChange, ActionError> {
        self.require(self.status == ActionStatus::Approved, "dispatch")?;
        let to = if current_plan_hash == self.plan_hash {
            ActionStatus::Dispatched
        } else {
            ActionStatus::Rejected {
                reason: "the plan changed after approval".to_owned(),
            }
        };
        Ok(self.move_to(to, now))
    }

    /// `Approved → Rejected` before dispatch, when a check other than the plan hash fails:
    /// a loop guard, a skill that is gone or changed risk, arguments no longer valid.
    pub fn reject(&mut self, reason: String, now: Timestamp) -> Result<ActionChange, ActionError> {
        self.require(self.status == ActionStatus::Approved, "reject")?;
        Ok(self.move_to(ActionStatus::Rejected { reason }, now))
    }

    /// `Dispatched → Succeeded` or `Dispatched → Failed`.
    pub fn complete(
        &mut self,
        result: Result<(), String>,
        now: Timestamp,
    ) -> Result<ActionChange, ActionError> {
        self.require(self.status == ActionStatus::Dispatched, "complete")?;
        let to = match result {
            Ok(()) => ActionStatus::Succeeded,
            Err(reason) => ActionStatus::Failed { reason },
        };
        Ok(self.move_to(to, now))
    }

    /// `Succeeded → Verified` or `Succeeded → VerifyFailed`.
    pub fn verified(&mut self, passed: bool, now: Timestamp) -> Result<ActionChange, ActionError> {
        self.require(self.status == ActionStatus::Succeeded, "verify")?;
        let to = if passed {
            ActionStatus::Verified
        } else {
            ActionStatus::VerifyFailed
        };
        Ok(self.move_to(to, now))
    }
}
