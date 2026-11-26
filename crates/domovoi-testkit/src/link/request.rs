//! The requests a fake agent link records.

use domovoi_core::ports::SkillDispatch;
use domovoi_core::{CheckKey, ExecutionId, HostId, JournalEpoch, ReadToolCall, TargetKey};
use jiff::Timestamp;

/// One request made through a `FakeAgentLink`.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkRequest {
    /// `run_tool`.
    Tool {
        /// Target host.
        host: HostId,
        /// The call.
        call: ReadToolCall,
        /// The deadline passed.
        deadline: Timestamp,
    },
    /// `run_skill`.
    Skill {
        /// Target host.
        host: HostId,
        /// The dispatch.
        dispatch: SkillDispatch,
        /// The deadline passed.
        deadline: Timestamp,
    },
    /// `run_check`.
    Check {
        /// Target host.
        host: HostId,
        /// The check.
        check: CheckKey,
        /// The target.
        target: Option<TargetKey>,
        /// The deadline passed.
        deadline: Timestamp,
    },
    /// `action_status`.
    Status {
        /// Target host.
        host: HostId,
        /// The executions asked about.
        ids: Vec<ExecutionId>,
        /// The epoch the caller believes the journal has.
        expected_epoch: JournalEpoch,
        /// The deadline passed.
        deadline: Timestamp,
    },
}
