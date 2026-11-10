//! The full incident as the follow-up answer sees it: the latest triage report, the
//! incident's actions and its recent timeline. The caller redacts the whole message.

use domovoi_core::ports::{ActionFilter, IncidentTimelineEntry, Page};
use domovoi_core::{ActionStatus, IncidentId, TriageOutcome};

use super::Shared;
use super::history::{CLEAR_KIND, FOLLOW_UP_KIND};
use super::tools::cut;

/// The most actions listed.
const MAX_ACTIONS: u16 = 20;

/// The most timeline lines listed, taking the newest.
const MAX_LINES: usize = 20;

/// The largest block, in bytes.
const BLOCK_BYTES: usize = 8 * 1024;

/// The latest triage report of `timeline`, as the model is shown it; empty when the
/// incident has none.
fn report_block(timeline: &[IncidentTimelineEntry]) -> String {
    let outcome = timeline
        .iter()
        .rev()
        .filter(|e| e.kind.starts_with("triage.") && e.kind != FOLLOW_UP_KIND)
        .find_map(|e| serde_json::from_value(e.detail.clone()).ok());
    let report = match outcome {
        Some(TriageOutcome::Report(found) | TriageOutcome::Cached { report: found, .. }) => {
            Some(found)
        }
        _ => None,
    };
    match report {
        None => String::new(),
        Some(found) => format!(
            "Latest triage report\nheadline: {headline}\ncause: {cause}\nverdict: {verdict}\nsummary: {summary}\nnext step: {next_step}",
            headline = found.headline,
            cause = found.cause,
            verdict = found.verdict.label(),
            summary = found.summary,
            next_step = found.next_step,
        ),
    }
}

/// The actions of `incident`, one per line: skill, arguments, status and, for a
/// rejected or failed one, the reason.
async fn actions_block(shared: &Shared, incident: IncidentId) -> String {
    let filter = ActionFilter {
        incident: Some(incident),
        ..ActionFilter::default()
    };
    let actions = match shared
        .deps
        .actions
        .list(&filter, &Page::first(MAX_ACTIONS))
        .await
    {
        Ok(found) => found.items,
        Err(err) => {
            tracing::warn!(%err, "could not list the incident actions for the follow-up context");
            Vec::new()
        }
    };
    if actions.is_empty() {
        return String::new();
    }
    let lines = actions
        .iter()
        .map(|action| {
            let reason = match action.status() {
                ActionStatus::Rejected { reason } | ActionStatus::Failed { reason } => {
                    format!(" reason: {reason}", reason = reason)
                }
                _ => String::new(),
            };
            let args = action
                .args()
                .iter()
                .map(|(name, value)| format!("{name}={value}", name = name, value = value))
                .collect::<Vec<String>>();
            let args = match args.is_empty() {
                true => String::new(),
                false => format!(" {list}", list = args.join(" ")),
            };
            format!(
                "{skill}{args} status: {status}{reason}",
                skill = action.skill(),
                args = args,
                status = action.status().name(),
                reason = reason,
            )
        })
        .collect::<Vec<String>>();
    format!("Actions of this incident\n{list}", list = lines.join("\n"))
}

/// The newest twenty timeline lines that are not follow-up entries, oldest first, as
/// `kind: summary`. Follow-up questions and clears are skipped because the conversation
/// itself is sent back to the model separately, and a cleared one must not resurface.
fn timeline_block(timeline: &[IncidentTimelineEntry]) -> String {
    let lines = timeline
        .iter()
        .skip(timeline.len().saturating_sub(MAX_LINES))
        .filter(|entry| entry.kind != FOLLOW_UP_KIND && entry.kind != CLEAR_KIND)
        .map(|entry| {
            format!(
                "{kind}: {summary}",
                kind = entry.kind,
                summary = entry.summary
            )
        })
        .collect::<Vec<String>>();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "Timeline of this incident, oldest first\n{list}",
        list = lines.join("\n")
    )
}

/// The incident as one block for a follow-up answer: report, actions and timeline, cut
/// to [`BLOCK_BYTES`].
pub(crate) async fn about(shared: &Shared, incident: IncidentId) -> String {
    let timeline = match shared.deps.incidents.timeline(incident).await {
        Ok(found) => found,
        Err(err) => {
            tracing::warn!(%err, "could not read the incident timeline for the follow-up context");
            Vec::new()
        }
    };
    let mut blocks = Vec::new();
    blocks.push(report_block(&timeline));
    blocks.push(actions_block(shared, incident).await);
    blocks.push(timeline_block(&timeline));
    let blocks = blocks
        .into_iter()
        .filter(|block| !block.is_empty())
        .collect::<Vec<String>>();
    if blocks.is_empty() {
        return String::new();
    }
    cut(&blocks.join("\n\n"), BLOCK_BYTES)
}
