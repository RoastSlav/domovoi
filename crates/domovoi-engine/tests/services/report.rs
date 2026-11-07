//! Reading the model's final answer as a report, with and without the by-hand checklist.

use domovoi_core::Verdict;
use serde_json::json;

use domovoi_engine::triage::parse_report;

/// One model answer, as text, with the given by-hand steps.
fn answer(steps: Vec<String>) -> String {
    let with = json!({
        "headline": "The AC was unplugged",
        "summary": "The power supply check failed on this host.",
        "cause": "The mains cable came out of the wall bracket.",
        "evidence": [],
        "verdict": "ACTION_NEEDED",
        "next_step": "Reconnect the cable and power the machine on.",
        "manual_steps": steps,
        "proposals": [],
        "details": "",
    });
    serde_json::to_string(&with).unwrap()
}

/// The same answer with the checklist left out, as old reports look.
fn answer_without_steps() -> String {
    let without = json!({
        "headline": "The AC was unplugged",
        "summary": "The power supply check failed on this host.",
        "cause": "The mains cable came out of the wall bracket.",
        "evidence": [],
        "verdict": "ACTION_NEEDED",
        "next_step": "Reconnect the cable and power the machine on.",
        "proposals": [],
        "details": "",
    });
    serde_json::to_string(&without).unwrap()
}

#[test]
fn parse_report_accepts_manual_steps_and_an_omitted_checklist() {
    let steps = vec![
        "Check the power cable is seated in the wall socket.".to_owned(),
        "Check the PSU inlet on the case.".to_owned(),
    ];
    let report = parse_report(&answer(steps.clone())).unwrap();
    assert_eq!(report.verdict, Verdict::ActionNeeded);
    assert_eq!(report.manual_steps, steps);

    let bare = parse_report(&answer_without_steps()).unwrap();
    assert!(bare.manual_steps.is_empty());
    assert_eq!(
        bare.next_step,
        "Reconnect the cable and power the machine on."
    );
}

#[test]
fn parse_report_rejects_more_than_five_manual_steps() {
    let six = vec!["check".to_owned(); 6];
    assert!(parse_report(&answer(six)).is_err());
}
