//! Reading the model's final answer as a report, with and without the by-hand checklist.

use std::sync::Arc;

use domovoi_core::TriageOutcome;
use domovoi_core::Verdict;
use domovoi_core::ports::{ActionRepo, Transaction};
use domovoi_core::triage::{MANUAL_STEP_MAX_CHARS, MAX_MANUAL_STEPS};
use domovoi_core::{ActionRequest, Principal};
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use serde_json::json;

use domovoi_engine::triage::{SYSTEM_PROMPT, parse_report};

use crate::harness::{SECRET, Setup, host_id, report_json, restart_skill, world, world_over};
use crate::uow_probe::RecordingUow;

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

/// Triage stores a new proposal and rotates a waiting action's chat token through the
/// action writer, so both land with their audit and journal rows in one commit.
#[tokio::test]
async fn triage_proposals_use_the_writer() {
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let w = world_over(Setup::default(), store, probe.clone()).await;
    let skill = restart_skill();
    w.link.set_skills(host_id(), vec![skill.clone()]);

    // A waiting action for the same skill and arguments as the first proposal.
    let params: std::collections::BTreeMap<String, String> = [("container", "nginx")]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    let args = skill.validate_args(&params).unwrap();
    let waiting = ActionRequest::propose(
        w.incident.id(),
        host_id(),
        &skill,
        &args,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    );
    let waiting_id = waiting.id();
    ActionRepo::insert(&*w.store, &waiting).await.unwrap();
    let before = waiting.chat_token_hash().copied();

    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([
            {"skill": "restart_container", "args": {"container": "nginx"}, "reason": "still restarting"},
            {"skill": "restart_container", "args": {"container": "redis"}, "reason": "also restarting"},
        ]),
    ));
    let outcome = w.run().await;
    let TriageOutcome::Report(report) = outcome else {
        panic!("a report, not {outcome:?}");
    };
    assert_eq!(
        report.proposals.len(),
        1,
        "the duplicate is a rotation, not a second action",
    );

    let commits = probe.commits();
    assert_eq!(
        commits.len(),
        2,
        "one proposal and one rotation, and nothing else",
    );
    for tx in &commits {
        assert_action_commit(tx);
    }
    let creations = commits
        .iter()
        .filter(|tx| tx.journal[0].payload["from"] == json!(null))
        .count();
    assert_eq!(creations, 1, "the new proposal is a creation commit");
    let rotations = commits
        .iter()
        .filter(|tx| tx.journal[0].payload["change"] == json!("chat_token_rotated"))
        .count();
    assert_eq!(
        rotations, 1,
        "the duplicate rotates the waiting action's token",
    );

    let stored = ActionRepo::get(&*w.store, waiting_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        stored.chat_token_hash().copied(),
        before,
        "the waiting action's chat token was replaced",
    );
}

/// One action commit: exactly one action write, one audit draft and one journal draft.
fn assert_action_commit(tx: &Transaction) {
    assert_eq!(tx.writes.len(), 1);
    assert!(matches!(
        tx.writes[0],
        domovoi_core::ports::StateWrite::Action { .. }
    ));
    assert_eq!(tx.audit.len(), 1);
    assert_eq!(tx.journal.len(), 1);
    assert_eq!(
        tx.journal[0].kind,
        domovoi_core::ports::JournalKind::ActionTransition
    );
}
