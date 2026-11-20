//! How each call the model made is recorded in the incident's tool-use list.

use domovoi_core::meta::MetaTool;
use domovoi_core::{ReadTool, SkillSummary};
use serde_json::Value;

use crate::triage::agent::ToolUse;

/// Longest model-supplied target stored in a timeline entry: the names are untrusted
/// text, so only a short piece of an unknown one is worth keeping.
const MAX_RAW_TOOL_USE: usize = 96;

/// The name every call outside the closed sets is recorded under.
const UNKNOWN: &str = "unknown tool";

/// A call recorded under the name `unknown tool`, keeping a short piece of what was asked.
pub(super) fn unknown(name: &str, refused: bool) -> ToolUse {
    ToolUse {
        tool: UNKNOWN.to_owned(),
        skill: None,
        failed: !refused,
        refused,
        raw: Some(cut(name)),
    }
}

/// The record of a call to `name` with `input`. `request_evidence` is recorded as the read
/// tool it names (with the skill for `run_read_skill`, only when the host catalogues it); the
/// other meta-tools under their own names; anything else as `unknown tool`.
pub(super) fn recorded(
    name: &str,
    input: &Value,
    skills: &[SkillSummary],
    failed: bool,
    refused: bool,
) -> ToolUse {
    let (tool, skill, raw) = if name == MetaTool::RequestEvidence.name() {
        let read = input.get("tool").and_then(Value::as_str).unwrap_or("");
        let skill = (read == ReadTool::RunReadSkill.name())
            .then(|| {
                input
                    .get("args")
                    .and_then(|args| args.get("skill"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .flatten();
        read_target(read, skill, skills)
    } else if MetaTool::from_name(name).is_some() {
        (name.to_owned(), None, None)
    } else {
        (UNKNOWN.to_owned(), None, Some(cut(name)))
    };
    ToolUse {
        tool,
        skill,
        failed,
        refused,
        raw,
    }
}

/// Recorded shape `(tool, skill, raw)` of a read. A name outside the closed
/// [`ReadTool::ALL`] collapses into one `unknown tool` row; a skill counts only when the host
/// catalogues it. The rejected target is kept cut short for the audit.
fn read_target(
    name: &str,
    skill: Option<String>,
    skills: &[SkillSummary],
) -> (String, Option<String>, Option<String>) {
    if ReadTool::from_name(name).is_none() {
        return (UNKNOWN.to_owned(), None, Some(cut(name)));
    }
    match skill {
        None => (name.to_owned(), None, None),
        Some(candidate) if skills.iter().any(|s| s.name.as_str() == candidate) => {
            (name.to_owned(), Some(candidate), None)
        }
        Some(candidate) => (name.to_owned(), None, Some(cut(&candidate))),
    }
}

fn cut(text: &str) -> String {
    text.chars().take(MAX_RAW_TOOL_USE).collect()
}
