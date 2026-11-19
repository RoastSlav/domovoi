//! The operation itself: its goal, coarse status, budgets and what it is pinned to.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::ValidationError;
use crate::id::{HostId, OperationId};
use crate::signal::{CheckKey, TargetKey};

use super::budget::Budgets;
use super::status::{OperationKind, OperationStatus};
use super::step::bounded;

/// Longest goal, in characters.
pub const MAX_GOAL_CHARS: usize = 500;
/// Most subjects an operation may be pinned to.
pub const MAX_PINNED: usize = 8;

/// A (host, check, target) an operation may act on. Executions outside the pinned subjects
/// are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedSubject {
    /// The host.
    pub host: HostId,
    /// The check.
    pub check: CheckKey,
    /// The target, if the check has one.
    pub target: Option<TargetKey>,
}

/// One durable piece of work on hosts, made of steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    /// The operation's id.
    pub id: OperationId,
    /// What it is for.
    pub kind: OperationKind,
    /// What it is trying to achieve, 1 to 500 characters, already redacted by the caller.
    pub goal: String,
    /// Where it stands.
    pub status: OperationStatus,
    /// The current verification cycle, starting at 0.
    pub cycle: u32,
    /// What it may spend and has spent.
    pub budgets: Budgets,
    /// What it may act on, 1 to 8 subjects.
    pub pinned: Vec<PinnedSubject>,
    /// The operation it was started from, if any.
    pub parent: Option<OperationId>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it left the queue.
    pub started_at: Option<Timestamp>,
    /// When it reached a terminal status.
    pub ended_at: Option<Timestamp>,
    /// When it last changed.
    pub updated_at: Timestamp,
}

impl Operation {
    /// A new, queued operation at cycle 0. Validates the goal and the pinned subjects.
    pub fn queued(
        id: OperationId,
        kind: OperationKind,
        goal: &str,
        pinned: Vec<PinnedSubject>,
        parent: Option<OperationId>,
        budgets: Budgets,
        now: Timestamp,
    ) -> Result<Self, ValidationError> {
        let operation = Self {
            id,
            kind,
            goal: goal.to_owned(),
            status: OperationStatus::Queued,
            cycle: 0,
            budgets,
            pinned,
            parent,
            created_at: now,
            started_at: None,
            ended_at: None,
            updated_at: now,
        };
        operation.validate()?;
        Ok(operation)
    }

    /// Checks the goal length and the number of pinned subjects.
    pub fn validate(&self) -> Result<(), ValidationError> {
        bounded("operation goal", &self.goal, MAX_GOAL_CHARS)?;
        if self.pinned.is_empty() || self.pinned.len() > MAX_PINNED {
            return Err(ValidationError::new(
                "operation pinned subjects",
                "must be 1 to 8",
            ));
        }
        Ok(())
    }

    /// Whether `(host, check, target)` is one of the pinned subjects.
    #[must_use]
    pub fn is_pinned(&self, host: HostId, check: &CheckKey, target: Option<&TargetKey>) -> bool {
        self.pinned
            .iter()
            .any(|p| p.host == host && &p.check == check && p.target.as_ref() == target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject() -> PinnedSubject {
        PinnedSubject {
            host: HostId::new(),
            check: CheckKey::parse("container.down").unwrap(),
            target: Some(TargetKey::parse("plex").unwrap()),
        }
    }

    fn build(goal: &str, pinned: Vec<PinnedSubject>) -> Result<Operation, ValidationError> {
        Operation::queued(
            OperationId::new(),
            OperationKind::Investigate,
            goal,
            pinned,
            None,
            Budgets::default(),
            Timestamp::UNIX_EPOCH,
        )
    }

    #[test]
    fn a_new_operation_is_queued_at_cycle_zero() {
        let op = build("find out why plex is down", vec![subject()]).unwrap();
        assert_eq!(op.status, OperationStatus::Queued);
        assert_eq!(op.cycle, 0);
        assert!(op.started_at.is_none());
    }

    #[test]
    fn goal_and_pinned_subjects_are_bounded() {
        assert!(build("", vec![subject()]).is_err());
        assert!(build(&"g".repeat(501), vec![subject()]).is_err());
        assert!(build("goal", vec![]).is_err());
        assert!(build("goal", (0..9).map(|_| subject()).collect()).is_err());
    }

    #[test]
    fn only_pinned_subjects_match() {
        let s = subject();
        let op = build("goal", vec![s.clone()]).unwrap();
        assert!(op.is_pinned(s.host, &s.check, s.target.as_ref()));
        assert!(!op.is_pinned(s.host, &s.check, None));
        assert!(!op.is_pinned(HostId::new(), &s.check, s.target.as_ref()));
    }
}
