//! Cards: what an incident looks like in chat, with and without triage.

use domovoi_core::{ActionId, CardButton, Risk, Severity, Verdict};
use domovoi_engine::cards::{CardLinks, build_card};
use domovoi_engine::render;
use domovoi_testkit::build::action;
use uuid::Uuid;

use crate::harness::{host, host_id, open, report, resolve, signal, ts};

fn action_id(n: u128) -> ActionId {
    ActionId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_2000 + n,
    ))
}

fn snapshot(name: &str, card: &domovoi_core::Card) {
    let rendered = render::card(card);
    assert!(rendered.lines().count() <= 8, "{rendered}");
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
        token: "tok_write".to_owned()
    }));
    assert!(card.buttons.contains(&CardButton::Deny {
        action: write.id(),
        token: "tok_write".to_owned()
    }));
    snapshot("write_proposal", &card);
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
