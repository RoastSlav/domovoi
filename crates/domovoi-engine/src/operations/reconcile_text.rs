//! What a journal answer settles an action to, the sentences the person sees, and the
//! machine-readable token kept in the journal row. One place says why a run did or did not
//! happen, for the journal path, for expiry before dispatch, and for a request the hub could
//! not hand over at all.

use domovoi_core::ports::{ActionAnswer, UnknownReason};

/// Positive evidence the agent's journal never received the request.
pub(crate) const NOT_RUN_REASON: &str = "the agent has no record of this run; nothing was started";
/// The journal recorded acceptance but not a start.
pub(crate) const NO_START_REASON: &str =
    "the agent recorded this run but not its start; it may not have run";
/// The agent restarted while the run was recorded as started.
pub(crate) const AGENT_RESTARTED_REASON: &str =
    "the agent restarted while this ran; check the host before running it again";
/// The journal lost its record, or the hub asked under a stale epoch.
pub(crate) const JOURNAL_LOST_REASON: &str =
    "the agent lost its record of this run; check the host before running it again";
/// The journal was reset since the run was dispatched.
pub(crate) const EPOCH_CHANGED_REASON: &str =
    "the agent's journal was reset since this ran; the result cannot be recovered";
/// The run was dispatched before the hub kept an epoch.
pub(crate) const EPOCH_UNKNOWN_REASON: &str =
    "this ran before the agent kept a journal; the result cannot be recovered";
/// The approval window closed before the action was sent.
pub(crate) const EXPIRED_REASON: &str = "the approval expired before the action was dispatched";
/// The token for the same, kept in the journal entry's `outcome_reason`.
pub(crate) const EXPIRED_TOKEN: &str = "expired";
/// The hub failed to hand the request to the agent before it was queued, so it was never
/// sent. This is the hub's own proof, not a journal answer.
pub(crate) const HANDOFF_FAILED_REASON: &str =
    "The hub could not hand the request to the agent; it was never sent";
/// The token for the same, kept in the journal entry's `outcome_reason`.
pub(crate) const HUB_NOT_HANDED_OFF_TOKEN: &str = "hub_not_handed_off";

/// What one journal answer or epoch comparison settles an action to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReconcileOutcome {
    /// The run finished and the recorded result is authoritative.
    Finished {
        /// Whether that result is a success.
        succeeded: bool,
        /// The agent's failure message, when it failed.
        detail: Option<String>,
    },
    /// The outcome cannot be established.
    Unknown(UnknownReason),
    /// Positive evidence the run never reached the agent.
    NeverRan,
    /// The hub proved that no request frame was ever queued for the agent.
    HandoffFailed,
}

impl ReconcileOutcome {
    /// The machine-readable token kept in the journal entry's `outcome_reason`.
    pub(crate) fn token(&self) -> &'static str {
        match self {
            Self::Finished { .. } => "finished",
            Self::NeverRan => "not_run",
            Self::HandoffFailed => HUB_NOT_HANDED_OFF_TOKEN,
            Self::Unknown(UnknownReason::NoStartRecord) => "no_start_record",
            Self::Unknown(UnknownReason::AgentRestarted) => "agent_restarted",
            Self::Unknown(UnknownReason::JournalLost) => "journal_lost",
            Self::Unknown(UnknownReason::EpochChanged) => "epoch_changed",
            Self::Unknown(UnknownReason::EpochUnknown) => "epoch_unknown",
        }
    }
}

/// The sentence for a journal reason the hub decided or the agent reported.
pub(crate) const fn reason_text(reason: UnknownReason) -> &'static str {
    match reason {
        UnknownReason::NoStartRecord => NO_START_REASON,
        UnknownReason::AgentRestarted => AGENT_RESTARTED_REASON,
        UnknownReason::JournalLost => JOURNAL_LOST_REASON,
        UnknownReason::EpochChanged => EPOCH_CHANGED_REASON,
        UnknownReason::EpochUnknown => EPOCH_UNKNOWN_REASON,
    }
}

/// What `answer` settles the run to, or `None` while it is still running and must be asked
/// about again.
pub(crate) fn from_answer(answer: ActionAnswer) -> Option<ReconcileOutcome> {
    match answer {
        ActionAnswer::Finished { succeeded, detail } => {
            Some(ReconcileOutcome::Finished { succeeded, detail })
        }
        ActionAnswer::Running => None,
        ActionAnswer::NotFound => Some(ReconcileOutcome::NeverRan),
        ActionAnswer::Unknown { reason } => Some(ReconcileOutcome::Unknown(reason)),
    }
}
