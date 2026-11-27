//! Running the model's reads: a parsed [`ReadToolCall`], answered from the store or through
//! the one evidence boundary, classified, redacted and cut.

use domovoi_core::ports::{IncidentFilter, Page};
use domovoi_core::{HostId, Incident, IncidentId, ReadToolCall};
use jiff::{SignedDuration, Timestamp};

use super::TriageDeps;
use crate::evidence::{EvidenceError, EvidenceRequest};

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

/// What one read came to, for the model and for the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EvidenceClass {
    /// The read ran and answered; an empty answer is a known empty.
    Observed,
    /// The host's agent was not there or speaks another version; the text says which.
    Unavailable(String),
    /// The read timed out, or the transport or the store failed; the text says how.
    Failed(String),
    /// The read was not allowed as asked: a skill that is missing, not read-only, disabled
    /// or given arguments its manifest rejects, or a spent budget.
    Refused(String),
}

impl EvidenceClass {
    /// The reason a read gave no evidence, `None` when it was observed.
    pub(crate) fn reason(&self) -> Option<&str> {
        match self {
            Self::Observed => None,
            Self::Unavailable(text) | Self::Failed(text) | Self::Refused(text) => Some(text),
        }
    }

    /// The same class with its reason passed through `redact`.
    fn redacted(self, redact: impl Fn(&str) -> String) -> Self {
        match self {
            Self::Observed => Self::Observed,
            Self::Unavailable(text) => Self::Unavailable(redact(&text)),
            Self::Failed(text) => Self::Failed(redact(&text)),
            Self::Refused(text) => Self::Refused(redact(&text)),
        }
    }
}

/// One classified read: the text the model is shown and what kind of answer it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolOutcome {
    /// What the read shows, exactly the text a read has always given.
    pub(crate) result: ToolResult,
    /// What the read came to.
    pub(crate) class: EvidenceClass,
}

/// The line added after an unknown read, so the model never reads it as a negative result.
pub const UNKNOWN_NOTE: &str = "This evidence is unknown, not a negative result: the read \
did not happen or did not finish. Do not treat it as \"nothing found\" or \"healthy\".";

/// Runs a parsed call. Agent calls are bounded by `run_deadline` and by [`TOOL_DEADLINE`],
/// whichever comes first.
pub(crate) async fn run(
    deps: &TriageDeps,
    scope: Scope,
    call: ReadToolCall,
    run_deadline: Timestamp,
) -> ToolResult {
    run_classified(deps, scope, call, run_deadline).await.result
}

/// Runs a parsed call and says what it came to. The text is the one [`run`] gives.
pub(crate) async fn run_classified(
    deps: &TriageDeps,
    scope: Scope,
    call: ReadToolCall,
    run_deadline: Timestamp,
) -> ToolOutcome {
    let (class, text) = read_redacted(deps, scope, call, run_deadline).await;
    let result = match class {
        EvidenceClass::Observed => finish(deps, &text, false),
        _ => finish(deps, &format!("error: {text}"), true),
    };
    ToolOutcome { result, class }
}

/// Runs a parsed call and returns its class and its redacted, uncut text: the output when it
/// was observed, otherwise the reason (which the class carries too).
pub(crate) async fn read_redacted(
    deps: &TriageDeps,
    scope: Scope,
    call: ReadToolCall,
    run_deadline: Timestamp,
) -> (EvidenceClass, String) {
    match execute(deps, scope, call, run_deadline).await {
        Ok(text) => (EvidenceClass::Observed, deps.redactor.redact(&text)),
        Err(class) => {
            let class = class.redacted(|text| deps.redactor.redact(text));
            let text = class.reason().unwrap_or_default().to_owned();
            (class, text)
        }
    }
}

/// `text` redacted and cut to [`MAX_RESULT_BYTES`], as a tool result.
pub(crate) fn finish(deps: &TriageDeps, text: &str, is_error: bool) -> ToolResult {
    let redacted = deps.redactor.redact(text);
    ToolResult {
        content: cut(&redacted, MAX_RESULT_BYTES),
        is_error,
    }
}

/// The error every tool path returns when the run ran out of time around it.
const TIMEOUT_TEXT: &str = "the tool did not finish before the run deadline";

/// Runs `work` under `deadline`, dropping it when the deadline passes so nothing keeps
/// running after the run is over. Every tool path uses this: a slow repository and a slow
/// agent are bounded the same way.
async fn bounded<F>(
    deps: &TriageDeps,
    deadline: Timestamp,
    work: F,
) -> Result<F::Output, EvidenceClass>
where
    F: std::future::Future,
{
    let left = deadline.duration_since(deps.clock.now());
    if left <= SignedDuration::ZERO {
        return Err(EvidenceClass::Failed(TIMEOUT_TEXT.to_owned()));
    }
    tokio::time::timeout(left.unsigned_abs(), work)
        .await
        .map_err(|_| EvidenceClass::Failed(TIMEOUT_TEXT.to_owned()))
}

fn store_failed(error: impl std::fmt::Display) -> EvidenceClass {
    EvidenceClass::Failed(error.to_string())
}

async fn execute(
    deps: &TriageDeps,
    scope: Scope,
    call: ReadToolCall,
    run_deadline: Timestamp,
) -> Result<String, EvidenceClass> {
    match &call {
        ReadToolCall::IncidentHistory { limit } => {
            let filter = IncidentFilter {
                host: Some(scope.host),
                ..IncidentFilter::default()
            };
            let page = bounded(
                deps,
                run_deadline,
                deps.incidents.list(&filter, &Page::first(*limit)),
            )
            .await?
            .map_err(store_failed)?;
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
            let hits = bounded(
                deps,
                run_deadline,
                deps.notes.search(query, Some(scope.host), *limit),
            )
            .await?
            .map_err(store_failed)?;
            Ok(if hits.is_empty() {
                "No matching notes.".to_owned()
            } else {
                hits.iter()
                    .map(|h| format!("- {}: {}", h.note.title, h.snippet))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        _ => agent(deps, scope, call, run_deadline).await,
    }
}

/// How an evidence boundary error is classed: not there is unavailable, a wrong request is
/// refused, anything else failed.
fn classify(error: EvidenceError) -> EvidenceClass {
    let text = error.to_string();
    match error {
        EvidenceError::NotConnected | EvidenceError::Incompatible => {
            EvidenceClass::Unavailable(text)
        }
        EvidenceError::NotEnabled(_) => EvidenceClass::Refused(text),
        EvidenceError::Timeout | EvidenceError::Transport(_) => EvidenceClass::Failed(text),
    }
}

/// Runs one read through the evidence boundary, within this run's deadline and the
/// per-tool limit, whichever comes first.
async fn agent(
    deps: &TriageDeps,
    scope: Scope,
    call: ReadToolCall,
    run_deadline: Timestamp,
) -> Result<String, EvidenceClass> {
    let now = deps.clock.now();
    let per_tool = now.checked_add(TOOL_DEADLINE).unwrap_or(now);
    // The per-tool limit is an extra safety net; the run's deadline always wins.
    let deadline = std::cmp::min(per_tool, run_deadline);
    let request = EvidenceRequest::for_host(scope.host, call, deadline);
    let out = bounded(deps, deadline, deps.evidence.collect_evidence(request))
        .await?
        .map_err(classify)?;
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
        priority: None,
    });
    calls.push(ReadToolCall::HostOverview);
    calls
}

#[cfg(test)]
mod tests {
    use super::{MAX_RESULT_BYTES, TOOL_DEADLINE};
    use domovoi_core::capability::builtin::{READ_OUTPUT_LIMIT, READ_TIMEOUT};

    #[test]
    fn builtin_read_limits_match_triage() {
        assert_eq!(READ_TIMEOUT, TOOL_DEADLINE);
        assert_eq!(READ_OUTPUT_LIMIT as usize, MAX_RESULT_BYTES);
    }
}
