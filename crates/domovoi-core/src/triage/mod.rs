//! Triage: the read-only tools the model may use and the report it must return.

mod claims;
mod coverage;
mod report;
mod security;
mod tool_call;
mod tool_schema;
mod tools;
#[cfg(test)]
mod triage_tests;

pub use claims::{NOT_ESTABLISHED, RECORDED_ACTION, neutral_question, question_asserts_a_claim};
pub use coverage::{
    SECURITY_EVENT_KIND, SEEN_UNTIL_KEY, is_security_event, reports_security_events,
    uncovered_events,
};
pub use report::{
    ActionProposal, EvidenceRef, HEADLINE_MAX_CHARS, MANUAL_STEP_MAX_CHARS, MAX_EVIDENCE,
    MAX_MANUAL_STEPS, MAX_PROPOSALS, NEXT_STEP_MAX_CHARS, TriageBudget, TriageOutcome,
    TriageReport,
};
pub use security::{
    NOT_DECIDED, NOT_REUSED, NOT_VERIFIED, cached_summary, current_episode, is_security_check,
    show_timeline,
};
pub use tool_call::{
    MAX_DEVICE_CHARS, MAX_HISTORY, MAX_LINES, MAX_NAME_CHARS, MAX_NOTES, MAX_QUERY_CHARS,
    MAX_SINCE_MINUTES, MAX_SKILL_ARGS, ReadToolCall, ToolArgError,
};
pub use tool_schema::argument_summary;
pub use tools::ReadTool;

/// The timeline kind a follow-up question and its answer are recorded under.
pub const FOLLOW_UP_KIND: &str = "triage.followup";

/// The timeline kind that cuts earlier questions and answers out of the conversation the
/// model sees.
pub const CLEAR_KIND: &str = "followup.cleared";
