//! Requests the hub sends to an agent, and their results.
//!
//! Each result frame's envelope `reply_to` is the `id` of the request it answers.

use std::collections::BTreeMap;

use domovoi_core::ports::{CheckRecheck, SkillDispatch, SkillOutput, ToolOutput};
use domovoi_core::{
    ActionId, CheckKey, Evidence, PlanHash, ReadToolCall, SignalStatus, SkillName, TargetKey,
};
use jiff::Timestamp;

use crate::bytes::Sha256Bytes;

/// Run one read tool and reply `tool_result` before `deadline`.
///
/// The agent re-parses `call` with the same bounds the hub applied, so an out-of-range
/// or unknown argument is rejected on arrival.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunTool {
    /// The tool and its arguments: `{"tool": "<name>", ...arguments}`.
    #[serde(with = "read_tool_call")]
    pub call: ReadToolCall,
    /// When the hub stops waiting.
    pub deadline: Timestamp,
}

/// The answer to `run_tool`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    /// What happened.
    pub outcome: ToolOutcome,
}

/// A tool's output or failure: `{"ok": {..}}` or `{"err": {..}}`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    /// The tool ran.
    Ok(ToolSuccess),
    /// The tool did not run or failed.
    Err(ToolFailure),
}

/// What a read tool returned.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolSuccess {
    /// Output text, redacted by the agent.
    pub text: String,
    /// Whether the agent cut the output at its cap.
    pub truncated: bool,
}

/// Why a read tool did not produce output.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolFailure {
    /// The failure class.
    pub code: ToolFailureCode,
    /// A short, redacted explanation.
    pub message: String,
}

/// A read tool failure class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureCode {
    /// The agent does not offer this tool.
    UnknownTool,
    /// The arguments failed the agent's checks.
    InvalidArgs,
    /// The tool's backend (Docker, journald) is not available.
    Unavailable,
    /// The deadline passed.
    Timeout,
    /// The tool started but failed.
    Exec,
    /// A code this build does not know.
    #[serde(other)]
    Unknown,
}

/// Run an approved action's skill and reply `skill_result` before `deadline`.
///
/// The agent never trusts this frame alone: it looks up `skill` in its own manifests,
/// checks the risk against its policy, validates `args` against the manifest's params
/// and recomputes the plan hash. Any mismatch is a `skill_result` error, not a run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunSkill {
    /// The action being executed.
    pub action_id: ActionId,
    /// The skill to run.
    pub skill: SkillName,
    /// Raw argument values, validated again by the agent.
    pub args: BTreeMap<String, String>,
    /// The plan hash the human approval bound to.
    pub plan_hash: PlanHash,
    /// When the hub stops waiting.
    pub deadline: Timestamp,
}

impl RunSkill {
    /// The frame for a dispatched action.
    #[must_use]
    pub fn from_dispatch(dispatch: &SkillDispatch, deadline: Timestamp) -> Self {
        Self {
            action_id: dispatch.action,
            skill: dispatch.skill.clone(),
            args: dispatch.args.to_map(),
            plan_hash: dispatch.plan_hash,
            deadline,
        }
    }
}

/// The answer to `run_skill`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillResult {
    /// What happened.
    pub outcome: SkillOutcome,
}

/// A skill run's output or the reason it did not run: `{"ok": {..}}` or `{"err": {..}}`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillOutcome {
    /// The skill ran (it may still have exited non-zero).
    Ok(SkillSuccess),
    /// The skill did not run, or was killed.
    Err(SkillFailure),
}

/// What a skill run produced.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillSuccess {
    /// Exit code, or `None` when killed by a signal.
    pub exit_code: Option<i32>,
    /// Standard output, redacted and capped.
    pub stdout: String,
    /// Standard error, redacted and capped.
    pub stderr: String,
    /// Whether either stream was cut.
    pub truncated: bool,
    /// Wall-clock run time in milliseconds.
    pub duration_ms: u64,
    /// SHA-256 of the argv the agent ran, for the audit log.
    pub argv_hash: Sha256Bytes,
}

/// Why a skill did not run to completion.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillFailure {
    /// The failure class.
    pub code: SkillFailureCode,
    /// A short, redacted explanation.
    pub message: String,
}

/// A skill failure class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillFailureCode {
    /// The agent has no skill with this name.
    NotFound,
    /// The skill's risk is not allowed by the agent's policy.
    RiskNotAllowed,
    /// The recomputed plan hash differs from the approved one.
    PlanMismatch,
    /// The arguments failed the agent's manifest.
    InvalidArgs,
    /// The deadline passed and the run was killed.
    Timeout,
    /// The process could not be started or failed to run.
    Exec,
    /// A code this build does not know.
    #[serde(other)]
    Unknown,
}

/// Re-run one check now and reply `check_result` before `deadline`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunCheck {
    /// The check to run.
    pub check: CheckKey,
    /// Restrict it to one target; `None` runs it for every target.
    pub target: Option<TargetKey>,
    /// When the hub stops waiting.
    pub deadline: Timestamp,
}

/// The answer to `run_check`: one observation per target checked.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CheckResult {
    /// The fresh observations.
    pub observations: Vec<CheckObservation>,
}

/// The state of one check target right now.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CheckObservation {
    /// The target, or `None` for a host-wide check.
    pub target: Option<TargetKey>,
    /// Whether the condition holds.
    pub status: SignalStatus,
    /// One-line description.
    pub summary: String,
    /// Fresh evidence.
    pub evidence: Evidence,
    /// When the check ran.
    pub observed_at: Timestamp,
}

impl From<ToolOutput> for ToolSuccess {
    fn from(out: ToolOutput) -> Self {
        Self {
            text: out.text,
            truncated: out.truncated,
        }
    }
}

impl From<ToolSuccess> for ToolOutput {
    fn from(out: ToolSuccess) -> Self {
        Self {
            text: out.text,
            truncated: out.truncated,
        }
    }
}

impl From<SkillOutput> for SkillSuccess {
    fn from(out: SkillOutput) -> Self {
        Self {
            exit_code: out.exit_code,
            stdout: out.stdout,
            stderr: out.stderr,
            truncated: out.truncated,
            duration_ms: out.duration_ms,
            argv_hash: Sha256Bytes(out.argv_hash),
        }
    }
}

impl From<SkillSuccess> for SkillOutput {
    fn from(out: SkillSuccess) -> Self {
        Self {
            exit_code: out.exit_code,
            stdout: out.stdout,
            stderr: out.stderr,
            truncated: out.truncated,
            duration_ms: out.duration_ms,
            argv_hash: out.argv_hash.0,
        }
    }
}

impl From<CheckObservation> for CheckRecheck {
    fn from(obs: CheckObservation) -> Self {
        Self {
            status: obs.status,
            summary: obs.summary,
            evidence: obs.evidence,
            observed_at: obs.observed_at,
        }
    }
}

/// `ReadToolCall` serializes as `{"tool": .., ..}` and deserializes through
/// `ReadToolCall::from_json`, which applies the catalogue's bounds.
mod read_tool_call {
    use domovoi_core::ReadToolCall;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value;

    pub(super) fn serialize<S: Serializer>(call: &ReadToolCall, s: S) -> Result<S::Ok, S::Error> {
        call.serialize(s)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ReadToolCall, D::Error> {
        let mut object = match Value::deserialize(d)? {
            Value::Object(object) => object,
            _ => return Err(D::Error::custom("a tool call must be an object")),
        };
        let tool = match object.remove("tool") {
            Some(Value::String(tool)) => tool,
            _ => return Err(D::Error::custom("a tool call needs a string `tool` field")),
        };
        ReadToolCall::from_json(&tool, Value::Object(object)).map_err(D::Error::custom)
    }
}
