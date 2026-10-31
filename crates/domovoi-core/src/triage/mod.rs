//! Triage: the read-only tools the model may use and the report it must return.

mod report;
mod tool_call;
mod tool_schema;
mod tools;
#[cfg(test)]
mod triage_tests;

pub use report::{
    ActionProposal, EvidenceRef, HEADLINE_MAX_CHARS, MAX_EVIDENCE, MAX_PROPOSALS,
    NEXT_STEP_MAX_CHARS, TriageBudget, TriageOutcome, TriageReport,
};
pub use tool_call::{
    MAX_DEVICE_CHARS, MAX_HISTORY, MAX_LINES, MAX_NAME_CHARS, MAX_NOTES, MAX_QUERY_CHARS,
    MAX_SINCE_MINUTES, MAX_SKILL_ARGS, ReadToolCall, ToolArgError,
};
pub use tools::ReadTool;
