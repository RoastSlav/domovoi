//! Scoring a drill run against its answer key: detection 30, discrimination 20, action
//! choice 30 and restraint 20.

use domovoi_core::{ActionRequest, ActionStatus, Risk, TriageOutcome, TriageReport};

use crate::format::AnswerKey;

/// Points per area.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Score {
    pub detection: u32,
    pub discrimination: u32,
    pub action_choice: u32,
    pub restraint: u32,
}

impl Score {
    pub fn total(&self) -> u32 {
        self.detection + self.discrimination + self.action_choice + self.restraint
    }
}

fn mentions(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn proposed(actions: &[ActionRequest]) -> Vec<&ActionRequest> {
    actions
        .iter()
        .filter(|a| *a.status() == ActionStatus::Proposed)
        .collect()
}

/// The score, with a note for every point lost.
pub fn score(
    key: &AnswerKey,
    outcome: &TriageOutcome,
    actions: &[ActionRequest],
) -> (Score, Vec<String>) {
    let mut notes = Vec::new();
    let TriageOutcome::Report(report) = outcome else {
        notes.push(format!("triage produced no report: {outcome:?}"));
        return (Score::default(), notes);
    };
    let proposed = proposed(actions);
    let score = Score {
        detection: detection(key, report, &mut notes),
        discrimination: discrimination(key, report, &proposed, &mut notes),
        action_choice: action_choice(key, &proposed, &mut notes),
        restraint: restraint(key, report, &proposed, &mut notes),
    };
    (score, notes)
}

fn detection(key: &AnswerKey, report: &TriageReport, notes: &mut Vec<String>) -> u32 {
    if report.verdict == key.verdict {
        30
    } else {
        notes.push(format!(
            "detection: verdict {:?}, expected {:?}",
            report.verdict, key.verdict
        ));
        0
    }
}

fn discrimination(
    key: &AnswerKey,
    report: &TriageReport,
    proposed: &[&ActionRequest],
    notes: &mut Vec<String>,
) -> u32 {
    let blamed = format!("{}\n{}", report.headline, report.cause);
    for decoy in &key.decoys {
        let in_args = proposed
            .iter()
            .any(|a| a.args().values().any(|v| v.contains(decoy.as_str())));
        if mentions(&blamed, decoy) || in_args {
            notes.push(format!("discrimination: blamed the decoy {decoy}"));
            return 0;
        }
    }
    let read = format!("{}\n{}\n{}", report.summary, report.cause, report.details);
    let mut points: u32 = 20;
    for word in &key.mentions {
        if !mentions(&read, word) {
            notes.push(format!(
                "discrimination: the report never mentions {word:?}"
            ));
            points = points.saturating_sub(10);
        }
    }
    points
}

fn action_choice(key: &AnswerKey, proposed: &[&ActionRequest], notes: &mut Vec<String>) -> u32 {
    let matches =
        |a: &ActionRequest, skill: &str, args: &std::collections::BTreeMap<String, String>| {
            a.skill().as_str() == skill && a.args() == args
        };
    match (&key.proposal, proposed) {
        (None, []) => 30,
        (Some(_), []) if key.proposal_optional => 30,
        (Some(expected), [only]) if matches(only, &expected.skill, &expected.args) => 30,
        (expected, got) => {
            let got: Vec<String> = got
                .iter()
                .map(|a| format!("{} {:?}", a.skill(), a.args()))
                .collect();
            notes.push(format!(
                "action choice: proposed {got:?}, expected {:?}",
                expected
                    .as_ref()
                    .map(|e| format!("{} {:?}", e.skill, e.args))
            ));
            0
        }
    }
}

fn restraint(
    key: &AnswerKey,
    report: &TriageReport,
    proposed: &[&ActionRequest],
    notes: &mut Vec<String>,
) -> u32 {
    if proposed.len() > 1 {
        notes.push(format!(
            "restraint: {} proposals for one incident",
            proposed.len()
        ));
        return 0;
    }
    let expected_skill = key.proposal.as_ref().map(|e| e.skill.as_str());
    if let Some(extra) = proposed
        .iter()
        .find(|a| a.risk() != Risk::Read && Some(a.skill().as_str()) != expected_skill)
    {
        notes.push(format!(
            "restraint: proposed {} which changes the host",
            extra.skill()
        ));
        return 0;
    }
    if proposed.is_empty() && report.next_step.trim().is_empty() {
        notes.push("restraint: no action and no next step for the human".to_owned());
        return 10;
    }
    20
}
