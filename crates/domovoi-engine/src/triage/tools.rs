//! Running the model's tool calls: parsed into the closed [`ReadToolCall`] set, answered
//! from the store or through the agent link, redacted and cut.

use domovoi_core::ports::{IncidentFilter, Page};
use domovoi_core::{HostId, Incident, IncidentId, ReadToolCall, Risk, SkillSummary};
use jiff::SignedDuration;
use serde_json::Value;

use super::Shared;

/// Where tool calls run: a host, and the incident to leave out of history, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scope {
    pub(crate) host: HostId,
    pub(crate) incident: Option<IncidentId>,
}

impl Scope {
    /// The scope a triage or follow-up runs in: the incident's host, and the incident
    /// itself left out of its own history.
    #[must_use]
    pub(crate) fn of(incident: &Incident) -> Self {
        Self {
            host: incident.host(),
            incident: Some(incident.id()),
        }
    }
}

/// The largest tool result handed to the model, in bytes.
pub const MAX_RESULT_BYTES: usize = 8 * 1024;

/// How long one tool call on an agent may take.
pub const TOOL_DEADLINE: SignedDuration = SignedDuration::from_secs(15);

/// One answered tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolResult {
    /// Redacted output, at most [`MAX_RESULT_BYTES`].
    pub(crate) content: String,
    /// Whether the call failed.
    pub(crate) is_error: bool,
}

/// `text` cut to at most `max` bytes on a character boundary, with a note when cut.
pub(crate) fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    const NOTE: &str = "\n[output cut]";
    let mut end = max.saturating_sub(NOTE.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{NOTE}", &text[..end])
}

/// Runs the call the model named `name` with `input` within `scope`. Unknown tools and bad
/// arguments are answered with an error the model can read; nothing outside the read
/// tools can run.
pub(crate) async fn call(
    shared: &Shared,
    scope: Scope,
    skills: &[SkillSummary],
    name: &str,
    input: Value,
) -> ToolResult {
    match ReadToolCall::from_json(name, input) {
        Ok(call) => run(shared, scope, skills, call).await,
        Err(err) => finish(shared, &format!("error: {err}"), true),
    }
}

/// Runs a parsed call.
pub(crate) async fn run(
    shared: &Shared,
    scope: Scope,
    skills: &[SkillSummary],
    call: ReadToolCall,
) -> ToolResult {
    match execute(shared, scope, skills, call).await {
        Ok(text) => finish(shared, &text, false),
        Err(text) => finish(shared, &format!("error: {text}"), true),
    }
}

fn finish(shared: &Shared, text: &str, is_error: bool) -> ToolResult {
    let redacted = shared.deps.redactor.redact(text);
    ToolResult {
        content: cut(&redacted, MAX_RESULT_BYTES),
        is_error,
    }
}

async fn execute(
    shared: &Shared,
    scope: Scope,
    skills: &[SkillSummary],
    call: ReadToolCall,
) -> Result<String, String> {
    let deps = &shared.deps;
    match &call {
        ReadToolCall::IncidentHistory { limit } => {
            let filter = IncidentFilter {
                host: Some(scope.host),
                ..IncidentFilter::default()
            };
            let page = deps
                .incidents
                .list(&filter, &Page::first(*limit))
                .await
                .map_err(|e| e.to_string())?;
            let lines: Vec<String> = page
                .items
                .iter()
                .filter(|i| scope.incident.is_none_or(|current| i.id() != current))
                .map(|i| {
                    let verdict = i.verdict().map_or("no verdict", |v| v.label());
                    format!(
                        "- {} {} [{}] {} ({verdict})",
                        i.opened_at(),
                        i.check(),
                        i.severity(),
                        i.title()
                    )
                })
                .collect();
            Ok(if lines.is_empty() {
                "No earlier incidents on this host.".to_owned()
            } else {
                lines.join("\n")
            })
        }
        ReadToolCall::NotesSearch { query, limit } => {
            let hits = deps
                .notes
                .search(query, Some(scope.host), *limit)
                .await
                .map_err(|e| e.to_string())?;
            Ok(if hits.is_empty() {
                "No matching notes.".to_owned()
            } else {
                hits.iter()
                    .map(|h| format!("- {}: {}", h.note.title, h.snippet))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        ReadToolCall::RunReadSkill { skill, args } => {
            let summary = skills
                .iter()
                .find(|s| &s.name == skill)
                .ok_or_else(|| format!("skill `{skill}` is not installed on this host"))?;
            if summary.risk != Risk::Read {
                return Err(format!("skill `{skill}` is not read-only"));
            }
            summary.validate_args(args).map_err(|e| e.to_string())?;
            agent(shared, scope, call).await
        }
        _ => agent(shared, scope, call).await,
    }
}

async fn agent(shared: &Shared, scope: Scope, call: ReadToolCall) -> Result<String, String> {
    let deps = &shared.deps;
    let deadline = deps
        .clock
        .now()
        .checked_add(TOOL_DEADLINE)
        .unwrap_or(deps.clock.now());
    let out = deps
        .link
        .run_tool(scope.host, call, deadline)
        .await
        .map_err(|e| e.to_string())?;
    Ok(if out.truncated {
        format!("{}\n[the agent cut this output]", out.text)
    } else {
        out.text
    })
}

/// The read tools run up front for providers without tool calling: containers, the
/// target's logs or journal, and the host overview.
pub(crate) fn prefetch(incident: &Incident) -> Vec<ReadToolCall> {
    let check = incident.check().as_str();
    let target = incident.target().map(|t| t.as_str().to_owned());
    let mut calls = vec![ReadToolCall::DockerPs { all: true }];
    let container = check.starts_with("docker.") || check.starts_with("container.");
    let unit = check.starts_with("systemd.") || check.starts_with("service.");
    if let (true, Some(name)) = (container, &target) {
        calls.push(ReadToolCall::DockerLogs {
            container: name.clone(),
            lines: 100,
        });
    }
    calls.push(ReadToolCall::Journal {
        unit: if unit { target } else { None },
        since_minutes: 30,
        lines: 100,
        kernel: false,
    });
    calls.push(ReadToolCall::HostOverview);
    calls
}
