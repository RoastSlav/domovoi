//! Where one host's attempt stands.

/// The state of one install target.
///
/// Validated here, never by a SQL check, so a new state needs no migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetStatus {
    /// Waiting for its turn on the host.
    Queued,
    /// Relaying the archive and signature file to the agent.
    Transferring,
    /// The request is handed to the agent; no authoritative answer yet.
    Dispatched,
    /// The agent said it is done and the outputs match.
    Succeeded,
    /// The agent answered that it refused or failed; nothing changed (the agent is atomic).
    Failed,
    /// Proof that nothing was installed or removed.
    NotRun,
    /// Handed over, but no authoritative answer ever came.
    OutcomeUnknown,
    /// The owner cancelled it before it started.
    Cancelled,
}

impl TargetStatus {
    /// Every status, in lifecycle order.
    pub const ALL: [Self; 8] = [
        Self::Queued,
        Self::Transferring,
        Self::Dispatched,
        Self::Succeeded,
        Self::Failed,
        Self::NotRun,
        Self::OutcomeUnknown,
        Self::Cancelled,
    ];

    /// The stored text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Transferring => "transferring",
            Self::Dispatched => "dispatched",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::NotRun => "not_run",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Cancelled => "cancelled",
        }
    }

    /// Reads [`TargetStatus::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == text)
    }

    /// Whether the target will never change again on its own.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Transferring | Self::Dispatched)
    }
}

/// What a target write expects to find: the stored status and version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetGuard {
    /// The status the stored target must have.
    pub status: TargetStatus,
    /// The version the stored target must have.
    pub version: u64,
}
