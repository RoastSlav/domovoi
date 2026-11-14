//! Cards: what an incident looks like in chat, with and without triage.

use std::collections::BTreeMap;

use domovoi_core::{
    ActionId, ActionRequest, CardButton, Incident, ParamKind, ParamName, ParamSpec, Principal,
    Risk, Severity, Verdict,
};
use domovoi_engine::cards::{CardLinks, build_card};
use domovoi_engine::render;
use domovoi_testkit::build::action;
use jiff::SignedDuration;
use uuid::Uuid;

use crate::harness::{host, host_id, open, report, resolve, signal, ts};

fn action_id(n: u128) -> ActionId {
    ActionId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_2000 + n,
    ))
}

/// A proposed `restart_container` write with `pairs` as arguments, proposed at `at`.
fn proposed_with_args(
    incident: &Incident,
    pairs: &[(&str, &str)],
    at: jiff::Timestamp,
) -> ActionRequest {
    let mut manifest = domovoi_testkit::build::manifest("restart_container", Risk::Write).unwrap();
    manifest.params = pairs
        .iter()
        .map(|(name, _)| ParamSpec {
            name: ParamName::parse(name).unwrap(),
            description: format!("The {name} argument."),
            required: true,
            kind: ParamKind::String {
                pattern: "^[-a-z0-9_]+$".to_owned(),
                max_len: 1024,
            },
        })
        .collect();
    let skill = manifest.summary();
    let values: BTreeMap<String, String> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    let args = skill.validate_args(&values).unwrap();
    ActionRequest::propose(
        incident.id(),
        host_id(),
        &skill,
        &args,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        at,
    )
}

/// The first 6 lowercase hex characters of `action`'s plan hash.
fn plan_prefix(action: &ActionRequest) -> String {
    hex::encode(&action.plan_hash().as_bytes()[..3])
}

fn snapshot(name: &str, card: &domovoi_core::Card) {
    let rendered = render::card(card);
    assert!(rendered.lines().count() <= 9, "{rendered}");
    insta::assert_json_snapshot!(format!("{name}_card"), card);
    insta::assert_snapshot!(format!("{name}_markdown"), rendered);
}

#[test]
fn card_without_report_uses_severity_title_and_signal_evidence() {
    let at = ts("2026-10-31T09:00:00Z");
    let incident = open(Severity::Warning, at);
    let latest = signal(
        "container.restart_loop",
        Some("plex"),
        Severity::Warning,
        at,
    );
    let card = build_card(
        &incident,
        None,
        &[],
        &host(),
        &CardLinks::default(),
        Some(&latest),
    );
    assert_eq!(
        card.buttons,
        [CardButton::Ack, CardButton::Mute1h, CardButton::Details]
    );
    assert_eq!(card.bullets, ["restarts: 7", "window: 10m"]);
    snapshot("no_report", &card);
}

#[test]
fn card_with_monitor_report() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let report = report(Verdict::Monitor);
    let card = build_card(
        &incident,
        Some(&report),
        &[],
        &host(),
        &CardLinks::default(),
        None,
    );
    snapshot("monitor", &card);
}

#[test]
fn card_with_write_proposal_has_approve_and_deny() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let write = action(incident.id(), host_id(), Risk::Write)
        .unwrap()
        .with_id(action_id(1));
    let mut links = CardLinks::default();
    links.tokens.insert(write.id(), "tok_write".to_owned());
    let report = report(Verdict::ActionNeeded);
    let card = build_card(
        &incident,
        Some(&report),
        std::slice::from_ref(&write),
        &host(),
        &links,
        None,
    );
    assert!(card.buttons.contains(&CardButton::Approve {
        action: write.id(),
        token: "tok_write".to_owned(),
        plan_prefix: Some(plan_prefix(&write)),
    }));
    assert!(card.buttons.contains(&CardButton::Deny {
        action: write.id(),
        token: "tok_write".to_owned()
    }));
    assert_eq!(card.plans.len(), 1);
    snapshot("write_proposal", &card);
}

#[test]
fn card_lists_waiting_actions_with_skill_host_args_and_plan() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let write = proposed_with_args(
        &incident,
        &[("zeta", "1"), ("alpha", "2")],
        ts("2026-10-31T09:00:00Z"),
    )
    .with_id(action_id(3));
    let mut links = CardLinks::default();
    links.tokens.insert(write.id(), "tok_write".to_owned());
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        std::slice::from_ref(&write),
        &host(),
        &links,
        None,
    );

    assert_eq!(card.plans.len(), 1);
    let plan = &card.plans[0];
    assert_eq!(plan.action, write.id());
    assert_eq!(plan.skill, "restart_container");
    assert_eq!(plan.host, host());
    assert_eq!(plan.risk, Risk::Write);
    assert_eq!(
        plan.args,
        [
            ("alpha".to_owned(), "2".to_owned()),
            ("zeta".to_owned(), "1".to_owned())
        ]
    );
    assert_eq!(plan.plan_short, write.plan_hash().short());
    assert_eq!(plan.plan_short.len(), 8);
    assert_eq!(plan.expires_at, write.expires_at());
    assert_eq!(
        plan.args_text().as_deref(),
        Some("alpha=2, zeta=1"),
        "the plan shows the arguments in name order"
    );
    assert!(card.buttons.contains(&CardButton::Approve {
        action: write.id(),
        token: "tok_write".to_owned(),
        plan_prefix: Some(plan_prefix(&write)),
    }));
    assert_eq!(plan_prefix(&write).len(), 6);
    assert!(card.validate().is_ok());
}

#[test]
fn card_with_long_arguments_links_to_the_web_instead_of_chat_buttons() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let long = "v".repeat(600);
    let write = proposed_with_args(
        &incident,
        &[("payload", long.as_str())],
        ts("2026-10-31T09:00:00Z"),
    )
    .with_id(action_id(4));
    let mut links = CardLinks::default();
    links.tokens.insert(write.id(), "tok_write".to_owned());
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        std::slice::from_ref(&write),
        &host(),
        &links,
        None,
    );

    assert!(
        !card
            .buttons
            .iter()
            .any(|b| matches!(b, CardButton::Approve { .. } | CardButton::Deny { .. })),
        "arguments too long to show must never be approved from chat"
    );
    assert!(
        card.buttons
            .contains(&CardButton::OpenInWeb { action: write.id() })
    );
    assert_eq!(card.plans.len(), 1);
    assert_eq!(card.plans[0].args_text(), None);
    let rendered = render::card(&card);
    assert!(
        rendered.contains("arguments too long to show here; review in the web UI"),
        "{rendered}"
    );
    assert!(
        rendered.contains("Waiting: restart_container on nas (write)"),
        "{rendered}"
    );
}

#[test]
fn card_limits_waiting_actions_to_the_three_oldest_proposed() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    // Newest first, and ids in the opposite order to the creation time, so only a sort by
    // `created_at` produces the right waiting list.
    let newest =
        proposed_with_args(&incident, &[], ts("2026-10-31T09:04:00Z")).with_id(action_id(5));
    let third =
        proposed_with_args(&incident, &[], ts("2026-10-31T09:03:00Z")).with_id(action_id(7));
    let second =
        proposed_with_args(&incident, &[], ts("2026-10-31T09:02:00Z")).with_id(action_id(6));
    let oldest =
        proposed_with_args(&incident, &[], ts("2026-10-31T09:01:00Z")).with_id(action_id(9));
    let mut links = CardLinks::default();
    for action in [&newest, &third, &second, &oldest] {
        links.tokens.insert(action.id(), "tok_write".to_owned());
    }
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        &[newest, third, second, oldest],
        &host(),
        &links,
        None,
    );

    assert_eq!(card.plans.len(), 3);
    assert_eq!(
        card.plans
            .iter()
            .map(|plan| plan.action)
            .collect::<Vec<_>>(),
        [action_id(9), action_id(6), action_id(7)]
    );
    assert!(
        !card
            .buttons
            .iter()
            .any(|b| matches!(b, CardButton::Approve { action, .. } if *action == action_id(5))),
        "the fourth waiting action gets no button"
    );
    assert!(card.validate().is_ok());
}

#[test]
fn markdown_card_names_the_waiting_action() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let write = action(incident.id(), host_id(), Risk::Write)
        .unwrap()
        .with_id(action_id(5));
    let mut links = CardLinks::default();
    links.tokens.insert(write.id(), "tok_write".to_owned());
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        std::slice::from_ref(&write),
        &host(),
        &links,
        None,
    );

    let rendered = render::card(&card);
    let expected = format!(
        "Waiting: restart_container on nas (write), no arguments · plan {}",
        write.plan_hash().short()
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let plan_at = lines
        .iter()
        .position(|line| *line == expected)
        .expect("the plan line is rendered");
    let first_bullet = lines
        .iter()
        .position(|line| line.starts_with("- "))
        .expect("the report's bullets are rendered");
    let next_at = lines
        .iter()
        .position(|line| line.starts_with("Next: "))
        .expect("the next step is rendered");
    assert!(
        plan_at < first_bullet,
        "the deterministic plan comes before the model's bullets:\n{rendered}"
    );
    assert!(
        plan_at < next_at,
        "the deterministic plan comes before the model's next step:\n{rendered}"
    );
    assert!(
        first_bullet < next_at,
        "bullets keep their order before the next step:\n{rendered}"
    );
    assert!(card.validate().is_ok());
}

#[test]
fn destructive_proposal_only_opens_in_web() {
    let incident = open(Severity::Critical, ts("2026-10-31T09:00:00Z"));
    let wipe = action(incident.id(), host_id(), Risk::Destructive)
        .unwrap()
        .with_id(action_id(2));
    let mut links = CardLinks::default();
    // Even with a token, a destructive action is never approvable from chat (INV-6).
    links.tokens.insert(wipe.id(), "tok_wipe".to_owned());
    let report = report(Verdict::ActionNeeded);
    let card = build_card(
        &incident,
        Some(&report),
        std::slice::from_ref(&wipe),
        &host(),
        &links,
        None,
    );
    assert!(
        !card
            .buttons
            .iter()
            .any(|b| matches!(b, CardButton::Approve { .. } | CardButton::Deny { .. }))
    );
    assert!(
        card.buttons
            .contains(&CardButton::OpenInWeb { action: wipe.id() })
    );
    snapshot("destructive_proposal", &card);
}

#[test]
fn missing_token_falls_back_to_open_in_web() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let write = action(incident.id(), host_id(), Risk::Write).unwrap();
    let card = build_card(
        &incident,
        None,
        std::slice::from_ref(&write),
        &host(),
        &CardLinks::default(),
        None,
    );
    assert!(
        card.buttons
            .contains(&CardButton::OpenInWeb { action: write.id() })
    );
    assert!(
        !card
            .buttons
            .iter()
            .any(|b| matches!(b, CardButton::Approve { .. }))
    );
}

#[test]
fn resolved_card_has_no_ack_or_mute() {
    let mut incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    resolve(&mut incident, ts("2026-10-31T09:42:00Z"));
    let report = report(Verdict::Fixed);
    let card = build_card(
        &incident,
        Some(&report),
        &[],
        &host(),
        &CardLinks::default(),
        None,
    );
    assert!(card.resolved);
    assert_eq!(card.buttons, [CardButton::Details]);
    snapshot("resolved", &card);
}

#[test]
fn long_headlines_are_cut() {
    let mut report = report(Verdict::Monitor);
    report.headline = "x".repeat(200);
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let card = build_card(
        &incident,
        Some(&report),
        &[],
        &host(),
        &CardLinks::default(),
        None,
    );
    assert_eq!(card.headline.chars().count(), 120);
    assert!(card.headline.ends_with('…'));
    assert!(card.validate().is_ok());
}
