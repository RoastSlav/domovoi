//! What an agent's journal says about a run: the answers a status query returns.

use crate::action::JournalEpoch;
use crate::id::ActionId;

/// What an agent's journal says about a run id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionAnswer {
    /// The run finished; `detail` is the agent's failure message when it failed.
    Finished {
        /// Whether the run's recorded result is a success.
        succeeded: bool,
        /// The agent's failure message, when the run failed.
        detail: Option<String>,
    },
    /// Still running in the agent instance that started it.
    Running,
    /// Positive evidence the request never reached the journal.
    NotFound,
    /// The journal cannot establish what happened.
    Unknown {
        /// Why.
        reason: UnknownReason,
    },
}

/// Why an outcome is unknown. The first three come from the agent's answer; the hub
/// decides the last two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    /// The journal recorded acceptance but never a start record.
    NoStartRecord,
    /// The agent restarted while the execution was recorded as started.
    AgentRestarted,
    /// The journal lost the record, or the hub's epoch is stale.
    JournalLost,
    /// The journal epoch the hub recorded at dispatch is not the live one.
    EpochChanged,
    /// The run was dispatched before the hub recorded an epoch.
    EpochUnknown,
}

/// One answer, keyed by the id that was asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAnswerEntry {
    /// The run the answer is about.
    pub execution: ActionId,
    /// What the journal knows.
    pub answer: ActionAnswer,
}

/// The agent's answers and the epoch it answered under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAnswers {
    /// The journal's actual epoch.
    pub epoch: JournalEpoch,
    /// One answer per requested id that the agent recognised.
    pub answers: Vec<ActionAnswerEntry>,
}
