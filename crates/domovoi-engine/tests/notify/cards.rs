//! Cards: what an incident looks like in chat, with and without triage.

use std::collections::BTreeMap;

use domovoi_core::{CardButton, ExecutionId, Risk, Severity, Verdict};
use domovoi_engine::cards::{CardLinks, WaitingStep, build_card};
use domovoi_engine::render;
use uuid::Uuid;

use crate::harness::{host, open, report, resolve, signal, ts};

fn execution_id(n: u128) -> ExecutionId {
    ExecutionId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_2000 + n,
    ))
}

/// A waiting `restart_container` step with `args` (already in name order), a fixed id and
/// `risk`, created at `at`.
fn waiting_step(n: u128, risk: Option<Risk>, args: &[(&str, &str)], at: &str) -> WaitingStep {
    WaitingStep {
        execution: execution_id(n),
        skill: "restart_container".to_owned(),
        host: host(),
        risk,
        args: args
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        plan_short: "abcd1234".to_owned(),
        plan_prefix: "abcd12".to_owned(),
        expires_at: ts("2026-11-01T09:00:00Z"),
        created_at: ts(at),
    }
}

/// Links for `steps`, each with a chat token when `with_tokens`.
fn step_links(steps: Vec<WaitingStep>, with_tokens: bool) -> CardLinks {
    let tokens = if with_tokens {
        steps
            .iter()
            .map(|s| (s.execution, "tok_write".to_owned()))
            .collect()
    } else {
        BTreeMap::new()
    };
    CardLinks {
        web_url: None,
        tokens,
        steps,
        uncovered: None,
    }
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
        &host(),
        &CardLinks::default(),
        None,
    );
    snapshot("monitor", &card);
}

#[test]
fn card_with_write_proposal_has_approve_and_deny() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let step = waiting_step(1, Some(Risk::Write), &[], "2026-10-31T09:00:00Z");
    let report = report(Verdict::ActionNeeded);
    let card = build_card(
        &incident,
        Some(&report),
        &host(),
        &step_links(vec![step.clone()], true),
        None,
    );
    assert!(card.buttons.contains(&CardButton::Approve {
        execution: step.execution,
        token: "tok_write".to_owned(),
        plan_prefix: Some("abcd12".to_owned()),
    }));
    assert!(card.buttons.contains(&CardButton::Deny {
        execution: step.execution,
        token: "tok_write".to_owned()
    }));
    assert_eq!(card.plans.len(), 1);
    snapshot("write_proposal", &card);
}

#[test]
fn card_lists_waiting_steps_with_skill_host_args_and_plan() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let step = waiting_step(
        3,
        Some(Risk::Write),
        &[("alpha", "2"), ("zeta", "1")],
        "2026-10-31T09:00:00Z",
    );
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        &host(),
        &step_links(vec![step.clone()], true),
        None,
    );

    assert_eq!(card.plans.len(), 1);
    let plan = &card.plans[0];
    assert_eq!(plan.execution, step.execution);
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
    assert_eq!(plan.plan_short, "abcd1234");
    assert_eq!(plan.expires_at, step.expires_at);
    assert_eq!(
        plan.args_text().as_deref(),
        Some("alpha=2, zeta=1"),
        "the plan shows the arguments in name order"
    );
    assert!(card.buttons.contains(&CardButton::Approve {
        execution: step.execution,
        token: "tok_write".to_owned(),
        plan_prefix: Some("abcd12".to_owned()),
    }));
    assert!(card.validate().is_ok());
}

#[test]
fn card_with_long_arguments_links_to_the_web_instead_of_chat_buttons() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let long = "v".repeat(600);
    let step = waiting_step(
        4,
        Some(Risk::Write),
        &[("payload", long.as_str())],
        "2026-10-31T09:00:00Z",
    );
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        &host(),
        &step_links(vec![step.clone()], true),
        None,
    );

    assert!(
        !card
            .buttons
            .iter()
            .any(|b| matches!(b, CardButton::Approve { .. } | CardButton::Deny { .. })),
        "arguments too long to show must never be approved from chat"
    );
    assert!(card.buttons.contains(&CardButton::OpenInWeb {
        execution: step.execution
    }));
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
fn card_limits_waiting_steps_to_the_three_oldest() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    // Newest first, and ids in the opposite order to the creation time, so only a sort by
    // creation time produces the right waiting list.
    let steps = vec![
        waiting_step(5, Some(Risk::Write), &[], "2026-10-31T09:04:00Z"),
        waiting_step(7, Some(Risk::Write), &[], "2026-10-31T09:03:00Z"),
        waiting_step(6, Some(Risk::Write), &[], "2026-10-31T09:02:00Z"),
        waiting_step(9, Some(Risk::Write), &[], "2026-10-31T09:01:00Z"),
    ];
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        &host(),
        &step_links(steps, true),
        None,
    );

    assert_eq!(card.plans.len(), 3);
    assert_eq!(
        card.plans
            .iter()
            .map(|plan| plan.execution)
            .collect::<Vec<_>>(),
        [execution_id(9), execution_id(6), execution_id(7)]
    );
    assert!(
        !card.buttons.iter().any(
            |b| matches!(b, CardButton::Approve { execution, .. } if *execution == execution_id(5))
        ),
        "the fourth waiting step gets no button"
    );
    assert!(card.validate().is_ok());
}

#[test]
fn markdown_card_names_the_waiting_step() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let step = waiting_step(5, Some(Risk::Write), &[], "2026-10-31T09:00:00Z");
    let card = build_card(
        &incident,
        Some(&report(Verdict::ActionNeeded)),
        &host(),
        &step_links(vec![step], true),
        None,
    );

    let rendered = render::card(&card);
    let expected = "Waiting: restart_container on nas (write), no arguments · plan abcd1234";
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
    let step = waiting_step(2, Some(Risk::Destructive), &[], "2026-10-31T09:00:00Z");
    let report = report(Verdict::ActionNeeded);
    // Even with a token, a destructive step is never approvable from chat (INV-6).
    let card = build_card(
        &incident,
        Some(&report),
        &host(),
        &step_links(vec![step.clone()], true),
        None,
    );
    assert!(
        !card
            .buttons
            .iter()
            .any(|b| matches!(b, CardButton::Approve { .. } | CardButton::Deny { .. }))
    );
    assert!(card.buttons.contains(&CardButton::OpenInWeb {
        execution: step.execution
    }));
    snapshot("destructive_proposal", &card);
}

#[test]
fn missing_token_falls_back_to_open_in_web() {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    let step = waiting_step(1, Some(Risk::Write), &[], "2026-10-31T09:00:00Z");
    let card = build_card(
        &incident,
        None,
        &host(),
        &step_links(vec![step.clone()], false),
        None,
    );
    assert!(card.buttons.contains(&CardButton::OpenInWeb {
        execution: step.execution
    }));
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
        &host(),
        &CardLinks::default(),
        None,
    );
    assert_eq!(card.headline.chars().count(), 120);
    assert!(card.headline.ends_with('…'));
    assert!(card.validate().is_ok());
}

fn card_of(links: &CardLinks) -> domovoi_core::Card {
    let incident = open(Severity::Warning, ts("2026-10-31T09:00:00Z"));
    build_card(&incident, None, &host(), links, None)
}

#[test]
fn card_lists_a_waiting_step_with_buttons_and_the_v2_plan_prefix() {
    let step = waiting_step(
        1,
        Some(Risk::Write),
        &[("container", "plex")],
        "2026-10-31T09:01:00Z",
    );
    let id = step.execution;
    let card = card_of(&step_links(vec![step], true));
    assert_eq!(card.plans.len(), 1);
    assert_eq!(card.plans[0].skill, "restart_container");
    assert_eq!(card.plans[0].plan_short, "abcd1234");
    assert!(card.buttons.iter().any(|b| matches!(
        b,
        CardButton::Approve { execution, plan_prefix: Some(p), .. } if *execution == id && p == "abcd12"
    )));
    assert!(
        card.buttons
            .iter()
            .any(|b| matches!(b, CardButton::Deny { execution, .. } if *execution == id))
    );
}

#[test]
fn card_offers_a_web_link_for_a_destructive_unknown_or_long_step() {
    let long = "x".repeat(600);
    let cases = [
        (
            waiting_step(1, Some(Risk::Destructive), &[], "2026-10-31T09:01:00Z"),
            true,
        ),
        (waiting_step(1, None, &[], "2026-10-31T09:01:00Z"), true),
        (
            waiting_step(
                1,
                Some(Risk::Write),
                &[("c", &long)],
                "2026-10-31T09:01:00Z",
            ),
            true,
        ),
        (
            waiting_step(1, Some(Risk::Write), &[], "2026-10-31T09:01:00Z"),
            false,
        ),
    ];
    for (step, tokens) in cases {
        let id = step.execution;
        let card = card_of(&step_links(vec![step], tokens));
        assert_eq!(card.plans.len(), 1, "the plan is still listed");
        assert!(
            card.buttons
                .contains(&CardButton::OpenInWeb { execution: id })
        );
        assert!(
            !card
                .buttons
                .iter()
                .any(|b| matches!(b, CardButton::Approve { .. } | CardButton::Deny { .. }))
        );
    }
}

#[test]
fn unknown_risk_is_shown_as_destructive_on_the_card() {
    let step = waiting_step(1, None, &[], "2026-10-31T09:01:00Z");
    let card = card_of(&step_links(vec![step], true));
    assert_eq!(card.plans[0].risk, Risk::Destructive);
}

#[test]
fn an_acked_or_muted_card_says_so_and_drops_the_buttons_that_would_be_refused() {
    use domovoi_core::{IncidentEvent, IncidentPolicy, UserId, apply};

    let at = ts("2026-10-31T09:00:00Z");
    let mut incident = open(Severity::Warning, at);
    let plain = |incident: &domovoi_core::Incident| {
        build_card(incident, None, &host(), &CardLinks::default(), None)
    };

    apply(
        &mut incident,
        IncidentEvent::Ack { by: UserId::new() },
        &IncidentPolicy::default(),
        at,
    )
    .unwrap();
    let acked = plain(&incident);
    assert_eq!(acked.status.as_deref(), Some("Acknowledged"));
    assert_eq!(acked.buttons, [CardButton::Mute1h, CardButton::Details]);

    let until = ts("2026-10-31T23:35:00Z");
    apply(
        &mut incident,
        IncidentEvent::Mute { until },
        &IncidentPolicy::default(),
        at,
    )
    .unwrap();
    let muted = plain(&incident);
    assert_eq!(muted.status.as_deref(), Some("Muted until 23:35 UTC"));
    assert_eq!(muted.buttons, [CardButton::Unmute, CardButton::Details]);
}
