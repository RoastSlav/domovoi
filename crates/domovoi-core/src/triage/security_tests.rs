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

fn measured() -> TriageReport {
    let mut r = report("MONITOR");
    r.headline = "Repeated sudo auth failures from Domovoi agent operations".to_owned();
    r.summary = "Likely expected behavior; the agent runs as kuker.".to_owned();
    r.next_step = "Add NOPASSWD to sudoers if appropriate.".to_owned();
    r
}

#[test]
fn a_stored_report_that_waves_the_event_away_is_withheld_when_shown() {
    let security = check("security.sudo_failure");
    for report in [measured(), {
        let mut r = measured();
        r.verdict = Verdict::Fixed;
        r
    }] {
        let mut outcome = TriageOutcome::Report(report);
        assert!(outcome.cap_security(&security, TITLE));
        let TriageOutcome::Report(r) = &outcome else {
            panic!("the kind is kept");
        };
        assert_eq!(r.verdict, Verdict::ActionNeeded);
        assert_eq!(r.cause, "Not established.");
        assert!(r.details.starts_with(NOT_ESTABLISHED));
        assert!(!r.next_step.contains("NOPASSWD") && !r.headline.contains("Domovoi"));
        assert!(
            !outcome.cap_security(&security, TITLE),
            "applying twice changes nothing"
        );
    }
    let mut stored = vec![entry(
        "triage.report",
        "Repeated sudo auth failures from Domovoi agent operations",
        serde_json::to_value(TriageOutcome::Report(measured())).expect("serializes"),
    )];
    show_timeline(&security, TITLE, &mut stored);
    assert!(
        stored[0].summary.starts_with("Not verified: ")
            || stored[0].summary.starts_with("Needs a person: ")
    );
    assert!(!stored[0].summary.contains("Domovoi"));
    let mut other = TriageOutcome::Report(measured());
    assert!(!other.cap_security(&check("disk.free"), TITLE));
}

#[test]
fn only_whole_words_and_the_conclusion_count() {
    let mut r = report("ACTION_NEEDED");
    r.headline = "An unexpected login".to_owned();
    r.summary = "Someone signed in from a new address.".to_owned();
    r.cause = "Not established.".to_owned();
    r.next_step = "Check who did this.".to_owned();
    r.details = "It might be routine, but nothing shows it.".to_owned();
    r.evidence.clear();
    assert!(!r.claims_without_evidence());
    assert!(!r.cap_security(&check("security.ssh_login"), TITLE));
    r.manual_steps = vec!["Run sudo visudo and set passwordless sudo.".to_owned()];
    assert!(r.claims_without_evidence());
}

fn blaming_domovoi() -> TriageReport {
    let mut r = report("MONITOR");
    r.headline = "Domovoi restarted nginx".to_owned();
    r.summary = "The Domovoi agent restarted the unit.".to_owned();
    r.cause = "The agent's restart.".to_owned();
    r.next_step = "Check the journal.".to_owned();
    r
}

#[test]
fn a_stored_report_that_names_a_recorded_action_is_not_withheld_when_shown_again() {
    let mut r = blaming_domovoi();
    r.record_action("operation 0190-aa: unit.restart unit=nginx.service");
    assert!(!r.cap_security(&check("security.sudo_failure"), TITLE));
    assert!(r.cause.starts_with("Recorded: operation 0190-aa"));
    // The same text without the engine's record line is the model's word alone.
    let mut bare = blaming_domovoi();
    assert!(bare.cap_security(&check("security.sudo_failure"), TITLE));
    assert_eq!(bare.cause, "Not established.");
}

#[test]
fn a_question_asserting_a_cause_is_detected_by_the_same_phrases() {
    use crate::triage::question_asserts_a_claim;
    assert!(question_asserts_a_claim(
        "Is this the agent's periodic check?",
        false
    ));
    assert!(!question_asserts_a_claim(
        "Is this the agent's periodic check?",
        true
    ));
    assert!(question_asserts_a_claim("Is this a routine login?", true));
    assert!(question_asserts_a_claim("May I set NOPASSWD?", true));
    assert!(!question_asserts_a_claim("Was this you?", false));
    assert!(!question_asserts_a_claim(
        "Was this an unexpected login?",
        false
    ));
}
