//! The state writes of autonomy: the pause switch, circuit-breaker resets and unknown-outcome
//! resolutions. A permit itself is written with its execution row, in the same commit.

use jiff::Timestamp;

use crate::id::ExecutionId;

/// How an unresolved unknown outcome was cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownResolution {
    /// An owner confirmed with their password.
    Owner,
    /// A later human-authorized run of the same host, contract and target settled verified.
    VerifiedByLaterRun,
}

impl UnknownResolution {
    /// The stored word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::VerifiedByLaterRun => "verified_by_later_run",
        }
    }
}

/// One change to autonomy state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermitWrite {
    /// Pauses new permits and the dispatch of permits already issued.
    Pause {
        /// Who paused.
        by: String,
        /// Why.
        reason: String,
        /// When.
        at: Timestamp,
    },
    /// Clears the pause.
    Resume {
        /// Who resumed.
        by: String,
        /// When.
        at: Timestamp,
    },
    /// Resets one rule's circuit breaker.
    ResetRule {
        /// The rule.
        rule: String,
        /// Who reset it.
        by: String,
        /// When.
        at: Timestamp,
    },
    /// Clears the unattended block of one `OutcomeUnknown` execution.
    ResolveUnknown {
        /// The execution.
        execution: ExecutionId,
        /// A user id, or `system:policy` for a verified later run.
        by: String,
        /// How it was cleared.
        how: UnknownResolution,
        /// An owner's note.
        note: Option<String>,
        /// When.
        at: Timestamp,
    },
}
