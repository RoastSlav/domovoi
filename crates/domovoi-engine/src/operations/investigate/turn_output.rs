//! What a reasoning turn stores as its output, and the report the card is built from.

use domovoi_core::{
    ContractId, EdgeKind, IncidentId, OperationSnapshot, StepId, StepIntent, StepKind, StepOutcome,
    StepRecord, TriageOutcome, TriageReport,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::triage::proposals::append_dropped;

/// The sentence a report ends with while a waiting step has no chat buttons.
pub const APPROVAL_POINTER: &str =
    "Proposed actions are waiting for your approval on the Operations page of the web UI.";

/// The most a turn's output may take, in bytes.
const MAX_OUTPUT: usize = 16 * 1024;

/// What the turn produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TurnKind {
    /// The model gave a report.
    Report,
    /// An earlier report answered.
    Cached,
    /// The run was not made.
    Skipped,
    /// The run failed.
    Failed,
}

/// The JSON a turn stores as its output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TurnOutput {
    /// Layout version.
    pub(crate) v: u8,
    /// What it is.
    pub(crate) kind: TurnKind,
    /// The incident a cached report came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) from: Option<IncidentId>,
    /// The report, for `report` and `cached`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) report: Option<TriageReport>,
    /// Why, for `skipped` and `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
    /// When a skipped run may be tried again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) retry_at: Option<Timestamp>,
    /// The "Proposals not offered" lines of the model's suggestions it never corrected.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) dropped: Vec<String>,
}

impl TurnOutput {
    /// An output of `kind` with nothing else set.
    pub(crate) fn of(kind: TurnKind) -> Self {
        Self {
            v: 1,
            kind,
            from: None,
            report: None,
            reason: None,
            retry_at: None,
            dropped: Vec::new(),
        }
    }

    /// The JSON, at most 16 KiB: the report's details are cut first, then its summary.
    pub(crate) fn to_json(&self) -> String {
        let mut output = self.clone();
        let mut text = serde_json::to_string(&output).unwrap_or_default();
        for field in [Field::Details, Field::Summary] {
            while text.len() > MAX_OUTPUT {
                let Some(report) = output.report.as_mut() else {
                    return text;
                };
                let target = match field {
                    Field::Details => &mut report.details,
                    Field::Summary => &mut report.summary,
                };
                if target.is_empty() {
                    break;
                }
                let over = text.len() - MAX_OUTPUT;
                let keep = target.chars().count().saturating_sub(over.max(1));
                *target = target.chars().take(keep).collect();
                text = serde_json::to_string(&output).unwrap_or_default();
            }
        }
        text
    }

    /// Reads a stored output back.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok().filter(|o: &Self| o.v == 1)
    }

    /// The one line that names what the turn came to.
    pub(crate) fn headline(&self) -> String {
        match (&self.report, &self.reason) {
            (Some(report), _) => report.headline.clone(),
            (None, Some(reason)) => reason.clone(),
            (None, None) => "no result".to_owned(),
        }
    }
}

/// Which report field is cut next.
#[derive(Clone, Copy)]
enum Field {
    Details,
    Summary,
}

/// The steps of `kind` that were caused by `turn` (or stored as its children), in ordinal
/// order.
pub(crate) fn caused_by(
    snapshot: &OperationSnapshot,
    turn: StepId,
    kind: StepKind,
) -> Vec<&StepRecord> {
    snapshot
        .steps
        .iter()
        .filter(|record| record.step.kind == kind)
        .filter(|record| {
            // A refused Execution step is stored without edges, so its parent names the turn.
            record.step.parent == Some(turn)
                || snapshot.edges.iter().any(|edge| {
                    edge.kind == EdgeKind::CausedBy
                        && edge.from == record.step.id
                        && edge.to == turn
                })
        })
        .collect()
}

/// The turn's stored output.
pub(crate) fn output_of(record: &StepRecord) -> Option<TurnOutput> {
    TurnOutput::parse(record.progress.output.as_deref()?)
}

/// Adds [`APPROVAL_POINTER`] to the report of `outcome`: some waiting step has no chat
/// buttons and is approved on the Operations page.
pub(crate) fn add_pointer(outcome: &mut TriageOutcome) {
    let (TriageOutcome::Report(report) | TriageOutcome::Cached { report, .. }) = outcome else {
        return;
    };
    if !report.details.trim().is_empty() {
        report.details.push_str("\n\n");
    }
    report.details.push_str(APPROVAL_POINTER);
}

/// The outcome the incident shows, rebuilt from the turn and the execution steps it caused.
///
/// The proposals kept are the ones whose Execution step is not refused; `dropped` (the
/// check failures, which have no step) and one line per refused step explain the others.
pub(crate) fn report_view(
    snapshot: &OperationSnapshot,
    turn: &StepRecord,
    dropped: &[String],
) -> Option<TriageOutcome> {
    let output = output_of(turn)?;
    let (mut report, from) = match (output.kind, output.report, output.from) {
        (TurnKind::Report, Some(report), _) => (report, None),
        (TurnKind::Cached, Some(report), Some(from)) => (report, Some(from)),
        (TurnKind::Skipped, _, _) => {
            return Some(TriageOutcome::Skipped {
                reason: output.reason.unwrap_or_default(),
            });
        }
        (TurnKind::Failed, _, _) => {
            return Some(TriageOutcome::Failed {
                reason: output.reason.unwrap_or_default(),
            });
        }
        _ => return None,
    };
    let executions = caused_by(snapshot, turn.step.id, StepKind::Execution);
    let mut lines: Vec<String> = dropped.to_vec();
    let mut refused: Vec<(&ContractId, &std::collections::BTreeMap<String, String>)> = Vec::new();
    let mut offered: Vec<(&ContractId, &std::collections::BTreeMap<String, String>)> = Vec::new();
    for record in &executions {
        let StepIntent::Execution(intent) = &record.step.intent else {
            continue;
        };
        if record.progress.outcome == StepOutcome::Refused {
            let reason = record.progress.reason.as_deref().unwrap_or("refused");
            let skill = intent
                .contract
                .skill_name()
                .map_or_else(|| intent.contract.to_string(), |name| name.to_string());
            lines.push(format!("{skill}: {reason}"));
            refused.push((&intent.contract, &intent.args));
        } else {
            offered.push((&intent.contract, &intent.args));
        }
    }
    report.proposals.retain(|proposal| {
        offered.iter().any(|(contract, args)| {
            contract
                .skill_name()
                .is_some_and(|name| name == proposal.skill)
                && **args == proposal.args
        })
    });
    append_dropped(&mut report, lines);
    Some(match from {
        Some(from) => TriageOutcome::Cached { report, from },
        None => TriageOutcome::Report(report),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(details: &str) -> TriageReport {
        serde_json::from_value(serde_json::json!({
            "headline": "Disk full", "summary": "s", "cause": "c", "verdict": "MONITOR",
            "next_step": "n", "details": details,
        }))
        .expect("a minimal report parses")
    }

    #[test]
    fn turn_output_round_trips_and_stays_under_16_kib() {
        let mut output = TurnOutput::of(TurnKind::Report);
        output.report = Some(report(&"x".repeat(40 * 1024)));
        let json = output.to_json();
        assert!(json.len() <= MAX_OUTPUT, "{} bytes", json.len());
        let back = TurnOutput::parse(&json).expect("it reads back");
        let kept = back.report.expect("a report");
        assert_eq!(kept.headline, "Disk full");
        assert_eq!(kept.summary, "s");
        assert!(!kept.details.is_empty());
        assert!(kept.details.len() < 40 * 1024);
    }

    #[test]
    fn a_small_output_is_stored_whole() {
        let mut output = TurnOutput::of(TurnKind::Skipped);
        output.reason = Some("the hourly limit is reached".to_owned());
        assert_eq!(TurnOutput::parse(&output.to_json()), Some(output));
    }
}
