//! The coarse state of an operation, why it paused and what it can wait for.

use serde::{Deserialize, Serialize};

use crate::id::StepId;

use super::budget::BudgetKind;

/// Why an operation is paused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PauseReason {
    /// A budget ran out.
    Budget {
        /// Which budget.
        exhausted: BudgetKind,
    },
    /// An execution may have run, and nobody knows whether it did.
    OutcomeUnknown {
        /// The Execution step whose outcome is unknown.
        step: StepId,
    },
    /// A person paused this operation. It needs no budget to resume.
    Manual,
    /// Every operation was paused by the global switch.
    GlobalPause,
    /// Something failed that the operation cannot recover from alone.
    Error {
        /// A short plain description.
        message: String,
    },
}

/// The coarse status of an operation. Fine detail lives in its steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationStatus {
    /// Created and waiting for a free slot.
    Queued,
    /// Working.
    Running,
    /// Waiting for a person to approve an execution.
    WaitingApproval,
    /// Waiting for a person to answer a question.
    WaitingUser,
    /// Waiting for an agent to come back.
    WaitingAgent,
    /// Waiting for a timer, such as the delay before a verification.
    WaitingTimer,
    /// Stopped until a person resumes it.
    Paused {
        /// Why it stopped.
        reason: PauseReason,
    },
    /// Finished and the problem is solved.
    Succeeded,
    /// Finished without solving the problem.
    Failed,
    /// Handed to a person.
    Escalated,
    /// Stopped by a person.
    Cancelled,
}

impl OperationStatus {
    /// The stored names, in declaration order.
    pub const NAMES: [&'static str; 11] = [
        "queued",
        "running",
        "waiting_approval",
        "waiting_user",
        "waiting_agent",
        "waiting_timer",
        "paused",
        "succeeded",
        "failed",
        "escalated",
        "cancelled",
    ];

    /// The stored name of this status, without any pause reason.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::WaitingUser => "waiting_user",
            Self::WaitingAgent => "waiting_agent",
            Self::WaitingTimer => "waiting_timer",
            Self::Paused { .. } => "paused",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Escalated => "escalated",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether the operation is over for good.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Escalated | Self::Cancelled
        )
    }

    /// Whether the operation is waiting for something outside itself.
    #[must_use]
    pub const fn is_waiting(&self) -> bool {
        matches!(
            self,
            Self::WaitingApproval | Self::WaitingUser | Self::WaitingAgent | Self::WaitingTimer
        )
    }

    /// The pause reason, when paused.
    #[must_use]
    pub const fn pause_reason(&self) -> Option<&PauseReason> {
        match self {
            Self::Paused { reason } => Some(reason),
            _ => None,
        }
    }

    /// Rebuilds a status from its stored name and, for `paused`, the stored reason. `None` for
    /// an unknown name, or for `paused` without a reason.
    #[must_use]
    pub fn from_parts(name: &str, pause: Option<PauseReason>) -> Option<Self> {
        Some(match name {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "waiting_approval" => Self::WaitingApproval,
            "waiting_user" => Self::WaitingUser,
            "waiting_agent" => Self::WaitingAgent,
            "waiting_timer" => Self::WaitingTimer,
            "paused" => Self::Paused { reason: pause? },
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "escalated" => Self::Escalated,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

/// What an operation can be waiting for, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitKind {
    /// A person's approval.
    Approval,
    /// A person's answer.
    User,
    /// An agent coming back.
    Agent,
    /// A timer.
    Timer,
}

/// What an operation is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// Find out what is wrong and, with approval, fix it.
    Investigate,
}

impl OperationKind {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Investigate => "investigate",
        }
    }

    /// Reads [`OperationKind::as_str`] back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "investigate" => Some(Self::Investigate),
            _ => None,
        }
    }
}

/// How a person or the model ends an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishOutcome {
    /// Solved.
    Succeeded,
    /// Not solved.
    Failed,
    /// Handed to a person.
    Escalated,
}

impl FinishOutcome {
    /// The status an operation ends in.
    #[must_use]
    pub const fn status(self) -> OperationStatus {
        match self {
            Self::Succeeded => OperationStatus::Succeeded,
            Self::Failed => OperationStatus::Failed,
            Self::Escalated => OperationStatus::Escalated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_status() -> Vec<OperationStatus> {
        vec![
            OperationStatus::Queued,
            OperationStatus::Running,
            OperationStatus::WaitingApproval,
            OperationStatus::WaitingUser,
            OperationStatus::WaitingAgent,
            OperationStatus::WaitingTimer,
            OperationStatus::Paused {
                reason: PauseReason::GlobalPause,
            },
            OperationStatus::Succeeded,
            OperationStatus::Failed,
            OperationStatus::Escalated,
            OperationStatus::Cancelled,
        ]
    }

    #[test]
    fn pause_reason_manual_round_trips_as_json() {
        let json = serde_json::to_string(&PauseReason::Manual).unwrap();
        assert_eq!(json, r#"{"type":"manual"}"#);
        let back: PauseReason = serde_json::from_str(&json).unwrap();
        assert_eq!(back, PauseReason::Manual);
    }

    #[test]
    fn operation_status_names_round_trip() {
        let all = every_status();
        assert_eq!(all.len(), OperationStatus::NAMES.len());
        for (status, name) in all.iter().zip(OperationStatus::NAMES) {
            assert_eq!(status.name(), name);
            let pause = status.pause_reason().cloned();
            assert_eq!(
                OperationStatus::from_parts(name, pause).as_ref(),
                Some(status)
            );
            let json = serde_json::to_string(status).unwrap();
            assert_eq!(
                &serde_json::from_str::<OperationStatus>(&json).unwrap(),
                status
            );
        }
        assert_eq!(OperationStatus::from_parts("paused", None), None);
        assert_eq!(OperationStatus::from_parts("sleeping", None), None);
    }

    #[test]
    fn terminal_and_waiting_statuses_are_told_apart() {
        let terminal: Vec<_> = every_status()
            .into_iter()
            .filter(OperationStatus::is_terminal)
            .map(|s| s.name())
            .collect();
        assert_eq!(terminal, ["succeeded", "failed", "escalated", "cancelled"]);
        let waiting = every_status().iter().filter(|s| s.is_waiting()).count();
        assert_eq!(waiting, 4);
    }

    #[test]
    fn operation_kinds_round_trip() {
        assert_eq!(
            OperationKind::parse(OperationKind::Investigate.as_str()),
            Some(OperationKind::Investigate)
        );
        assert_eq!(OperationKind::parse("repair"), None);
    }
}
