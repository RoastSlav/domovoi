//! Reading the model's final answer as a report, with and without the by-hand checklist.

use domovoi_core::TriageOutcome;
use domovoi_core::Verdict;
use domovoi_core::triage::{MANUAL_STEP_MAX_CHARS, MAX_MANUAL_STEPS};
use serde_json::json;

use domovoi_engine::triage::{SYSTEM_PROMPT, parse_report};

use crate::harness::{SECRET, Setup, world};

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

/// The schema entry for `manual_steps`, from its key up to the next one.
fn manual_step_schema() -> String {
    let start = SYSTEM_PROMPT.find("\"manual_steps\"").unwrap();
    let end = SYSTEM_PROMPT.find("\"proposals\"").unwrap();
    assert!(
        start < end,
        "manual_steps is listed before proposals in the schema"
    );
    SYSTEM_PROMPT[start..end].to_owned()
}

#[test]
fn the_triage_prompt_states_the_manual_step_limits() {
    let entry = manual_step_schema();
    assert!(
        entry.contains(&format!(r#""maxItems": {MAX_MANUAL_STEPS}"#)),
        "the prompt caps the checklist at {MAX_MANUAL_STEPS} steps",
    );
    assert!(
        entry.contains(&format!(r#""maxLength": {MANUAL_STEP_MAX_CHARS}"#)),
        "the prompt caps one step at {MANUAL_STEP_MAX_CHARS} characters",
    );
}

#[test]
fn the_triage_prompt_runs_a_read_skill_instead_of_proposing_one() {
    let rules: Vec<&str> = SYSTEM_PROMPT
        .lines()
        .filter(|line| line.trim_start().starts_with("- ") && line.contains("manual_steps"))
        .collect();
    assert_eq!(rules.len(), 1, "one rule about the by-hand checklist");
    let rule = rules[0];
    assert!(
        rule.contains("run_read_skill"),
        "the model runs the skill: {rule}"
    );
    assert!(
        !rule.contains("propose"),
        "a read must not become a proposal, which a human would have to approve: {rule}",
    );
}

/// What triage does with the checklist before it reaches the page: the steps are redacted,
/// and the ones that are empty afterwards go away, so the list never shows a blank item.
#[tokio::test]
async fn triage_drops_blank_manual_steps_and_redacts_a_step_that_quotes_a_secret() {
    let w = world(Setup::default()).await;
    let steps = [
        "Check the cable is seated in the wall bracket.".to_owned(),
        "   ".to_owned(),
        format!("The label on the PSU reads password {SECRET}."),
    ]
    .to_vec();
    w.primary.push_text(answer(steps));
    let outcome = w.run().await;

    let kept: Vec<String> = match &outcome {
        TriageOutcome::Report(report) => report.manual_steps.clone(),
        _ => Vec::new(),
    };
    assert_eq!(
        kept,
        vec![
            "Check the cable is seated in the wall bracket.".to_owned(),
            "The label on the PSU reads password [REDACTED].".to_owned(),
        ],
        "the blank step went away, the secret with it",
    );
}
