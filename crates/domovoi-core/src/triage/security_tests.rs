use serde_json::json;

use super::*;
use crate::id::IncidentId;

const TITLE: &str = "Failed sudo authentication";

fn check(name: &str) -> CheckKey {
    CheckKey::parse(name).expect("a check key")
}

fn report(verdict: &str) -> TriageReport {
    serde_json::from_value(json!({
        "headline": "Expected sudo authentication failure from Domovoi agent operations",
        "summary": "This is expected. No action required.",
        "cause": "The agent account, as in earlier incidents.",
        "verdict": verdict,
        "next_step": "No action required.",
        "evidence": [
            {"label": "before", "value": "same", "source": "incident_history"},
            {"label": "line", "value": "auth failure", "source": "journal"},
        ],
        "details": "reasoning",
    }))
    .expect("a report")
}

fn says_nothing_is_needed(report: &TriageReport) -> bool {
    [
        &report.headline,
        &report.summary,
        &report.cause,
        &report.next_step,
    ]
    .iter()
    .any(|text| text.contains("No action required") || text.starts_with("Expected"))
}

#[test]
fn the_text_agrees_with_a_capped_verdict() {
    let mut r = report("EXPECTED");
    assert!(r.cap_security(&check("security.sudo_failure"), TITLE));
    assert_eq!(r.verdict, Verdict::ActionNeeded);
    assert!(!says_nothing_is_needed(&r), "{r:?}");
    assert_eq!(r.headline, format!("Needs a person: {TITLE}"));
    assert!(r.details.starts_with(NOT_DECIDED));
    assert!(r.details.contains("not accepted"));
    assert_eq!(r.evidence.len(), 1, "a past incident is not evidence");
    r.validate().expect("still a valid report");
    // Applying it again changes nothing.
    assert!(!r.cap_security(&check("security.sudo_failure"), TITLE));
}

#[test]
fn a_report_capped_by_an_older_build_gets_agreeing_text() {
    let mut r = report("ACTION_NEEDED");
    r.details = format!("{NOT_DECIDED}\n\nreasoning");
    assert!(r.cap_security(&check("security.ssh_login"), TITLE));
    assert!(!says_nothing_is_needed(&r), "{r:?}");
    assert_eq!(r.details.matches(NOT_DECIDED).count(), 2 - 1);
}

#[test]
fn other_checks_are_left_alone() {
    let mut r = report("EXPECTED");
    assert!(!r.cap_security(&check("disk.free"), TITLE));
    assert_eq!(r.verdict, Verdict::Expected);
}

#[test]
fn a_reused_report_never_answers_a_security_event() {
    for verdict in ["EXPECTED", "MONITOR", "FIXED", "ACTION_NEEDED"] {
        let mut outcome = TriageOutcome::Cached {
            report: report(verdict),
            from: IncidentId::new(),
        };
        assert!(outcome.cap_security(&check("security.new_port"), TITLE));
        let TriageOutcome::Cached { report, .. } = &outcome else {
            panic!("the kind is kept");
        };
        assert_eq!(report.verdict, Verdict::ActionNeeded);
        assert!(report.evidence.is_empty());
        assert!(!says_nothing_is_needed(report));
    }
}

fn entry(kind: &str, summary: &str, detail: serde_json::Value) -> IncidentTimelineEntry {
    IncidentTimelineEntry {
        incident: IncidentId::new(),
        at: "2026-11-27T13:13:52Z".parse().expect("a time"),
        kind: kind.to_owned(),
        summary: summary.to_owned(),
        detail,
    }
}

#[test]
fn stored_entries_are_shown_capped_and_an_earlier_episode_is_not_current() {
    let stored = TriageOutcome::Cached {
        report: report("EXPECTED"),
        from: IncidentId::new(),
    };
    let mut entries = vec![
        entry(
            "triage.cached",
            "Expected sudo failure",
            serde_json::to_value(&stored).expect("serializes"),
        ),
        entry(
            "status.changed",
            "resolved → open",
            json!({"from": IncidentStatus::Resolved, "to": IncidentStatus::Open}),
        ),
    ];
    let security = check("security.sudo_failure");
    show_timeline(&security, TITLE, &mut entries);
    assert!(entries[0].summary.starts_with("Needs a person: "));
    assert!(!entries[0].detail.to_string().contains("\"EXPECTED\""));
    assert_eq!(current_episode(&security, &entries).len(), 1);
    assert_eq!(current_episode(&check("disk.free"), &entries).len(), 2);
}
