//! Checking a stored action before it becomes a usable one.
//!
//! A row's columns are decoded independently, so nothing in the store can tell whether they
//! agree. The rules live here, in the domain, and both the store's mapper and any other
//! loader use them; nothing is repaired on the way in.

use super::approval::{ApprovalProvenance, PlanHash};
use super::authorization::AuthorizationKind;
use super::request::{ActionStatus, PlanFamily, StoredAction};

/// Why approval evidence does not match the plan it claims to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalEvidenceError {
    /// A current approval exists but the displayed hash was not recorded.
    #[error("the approval has no displayed plan hash")]
    MissingDisplayedHash,
    /// The displayed hash belongs to a different plan.
    #[error("the displayed plan hash does not match the {family} plan")]
    MismatchedDisplayedHash {
        /// Which family the plan belongs to, `v1` or `v2`.
        family: &'static str,
    },
    /// The approval does not say where it was made.
    #[error("the approval has no provenance")]
    MissingProvenance,
}

/// The approval fields of one action and the plan they must describe.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ApprovalEvidence<'a> {
    /// Which family the plan belongs to.
    pub family: PlanFamily,
    /// The plan hash a human approved.
    pub plan_hash: PlanHash,
    /// The hash the approval carried, if one was recorded.
    pub approved_plan_hash: Option<PlanHash>,
    /// Where the approval was made, if that was recorded.
    pub provenance: Option<&'a ApprovalProvenance>,
}

impl ApprovalEvidence<'_> {
    /// Checks that the approval fields describe [`Self::plan_hash`] and nothing else.
    ///
    /// A legacy v1 approval predates the displayed hash and provenance and may have neither;
    /// a current approval has both. Either partial form is a contradiction, not an old row.
    pub(crate) fn check(self) -> Result<(), ApprovalEvidenceError> {
        match self.family {
            PlanFamily::V2 => {
                let Some(displayed) = self.approved_plan_hash else {
                    return Err(ApprovalEvidenceError::MissingDisplayedHash);
                };
                if displayed != self.plan_hash {
                    return Err(ApprovalEvidenceError::MismatchedDisplayedHash { family: "v2" });
                }
                if self.provenance.is_none() {
                    return Err(ApprovalEvidenceError::MissingProvenance);
                }
                Ok(())
            }
            PlanFamily::V1 => match (self.approved_plan_hash, self.provenance) {
                (None, None) => Ok(()),
                (Some(displayed), Some(_)) if displayed == self.plan_hash => Ok(()),
                (Some(_), Some(_)) => {
                    Err(ApprovalEvidenceError::MismatchedDisplayedHash { family: "v1" })
                }
                (Some(_), None) => Err(ApprovalEvidenceError::MissingProvenance),
                (None, Some(_)) => Err(ApprovalEvidenceError::MissingDisplayedHash),
            },
        }
    }
}

/// Why a [`StoredAction`] cannot become an [`ActionRequest`](super::ActionRequest).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoredActionError {
    /// A status before any decision carries approval fields it cannot have yet.
    #[error("an undecided action carries approval metadata")]
    UndecidedApproval,
    /// A decided action has no authorization case.
    #[error("a decided action ({status}) has no authorization case")]
    MissingAuthorization {
        /// The status name.
        status: &'static str,
    },
    /// A human authorization exists without the human approval behind it.
    #[error("a human-approved action has no approver")]
    MissingApprover,
    /// A policy row also carries a human approval.
    #[error("a policy action also carries a human approver")]
    PolicyWithApprover,
    /// At M1 a policy row is only accepted while it waits to be refused at dispatch.
    #[error("a policy action is only accepted while it is approved, not {status}")]
    PolicyNotApproved {
        /// The status name.
        status: &'static str,
    },
    /// The approval's displayed hash or provenance contradicts its plan.
    #[error(transparent)]
    Evidence(#[from] ApprovalEvidenceError),
}

/// Rejects a stored action whose status and approval fields disagree.
pub(crate) fn validate(stored: &StoredAction) -> Result<(), StoredActionError> {
    let status = stored.status.name();
    let decided = !matches!(
        stored.status,
        ActionStatus::Proposed | ActionStatus::Denied | ActionStatus::Expired
    );
    if !decided {
        if stored.approved_by.is_some()
            || stored.authorization_kind.is_some()
            || stored.approved_plan_hash.is_some()
            || stored.provenance.is_some()
        {
            return Err(StoredActionError::UndecidedApproval);
        }
        return Ok(());
    }
    let Some(kind) = stored.authorization_kind else {
        return Err(StoredActionError::MissingAuthorization { status });
    };
    match kind {
        AuthorizationKind::Human => {
            if stored.approved_by.is_none() {
                return Err(StoredActionError::MissingApprover);
            }
            let family = if stored.plan_hash_v2.is_some() {
                PlanFamily::V2
            } else {
                PlanFamily::V1
            };
            ApprovalEvidence {
                family,
                plan_hash: stored.plan_hash_v2.unwrap_or(stored.plan_hash),
                approved_plan_hash: stored.approved_plan_hash,
                provenance: stored.provenance.as_ref(),
            }
            .check()?;
            Ok(())
        }
        AuthorizationKind::Policy => {
            if stored.approved_by.is_some() {
                return Err(StoredActionError::PolicyWithApprover);
            }
            // No permit is issued in this milestone. The one shape the writer needs to
            // refuse with a named error is an approved row; a policy row claiming to have
            // run is not a valid execution and is refused instead of hydrated.
            if !matches!(stored.status, ActionStatus::Approved) {
                return Err(StoredActionError::PolicyNotApproved { status });
            }
            Ok(())
        }
    }
}
