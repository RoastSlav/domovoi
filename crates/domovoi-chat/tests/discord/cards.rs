use domovoi_chat::discord::DiscordSettings;
use domovoi_chat::discord::components::card_buttons;
use domovoi_chat::discord::embed::card_embed;
use domovoi_core::{
    Card, CardBadge, CardButton, CardPlan, ExecutionId, HostName, IncidentId, Risk, Severity,
    Verdict,
};
use serde_json::{Value, json};
use url::Url;

fn settings() -> DiscordSettings {
    DiscordSettings {
        channel_id: 1234,
        web_base: Url::parse("https://domovoi.example.net").unwrap(),
    }
}

fn incident() -> IncidentId {
    "01a19721-5d58-7277-a3eb-f866e4efb986".parse().unwrap()
}

fn execution(n: u8) -> ExecutionId {
    format!("01a19721-6000-7000-8000-0000000000{n:02}")
        .parse()
        .unwrap()
}

fn card(badge: CardBadge, buttons: Vec<CardButton>) -> Card {
    Card {
        incident: incident(),
        host: HostName::parse("nas").unwrap(),
        badge,
        headline: "Disk /srv is 93% full".to_owned(),
        bullets: vec![
            "93% used, 70 GB free".to_owned(),
            "Grew 40 GB since 02:00".to_owned(),
        ],
        next_step: Some("Prune old snapshots".to_owned()),
        buttons,
        plans: Vec::new(),
        details: Some("The nightly job kept every snapshot.".to_owned()),
        resolved: false,
    }
}

fn basic() -> Vec<CardButton> {
    vec![CardButton::Ack, CardButton::Mute1h, CardButton::Details]
}

fn token(c: char) -> String {
    c.to_string().repeat(64)
}

/// The message as Discord receives it: the embed and the component rows.
fn rendered(card: &Card) -> Value {
    json!({
        "embed": serde_json::to_value(card_embed(card, &settings())).unwrap(),
        "components": serde_json::to_value(card_buttons(card, &settings())).unwrap(),
    })
}

#[test]
fn verdict_badges() {
    for (name, verdict) in [
        ("action_needed", Verdict::ActionNeeded),
        ("monitor", Verdict::Monitor),
        ("expected", Verdict::Expected),
        ("fixed", Verdict::Fixed),
    ] {
        let card = card(CardBadge::Verdict(verdict), basic());
        insta::assert_json_snapshot!(format!("verdict_{name}"), rendered(&card));
    }
}

#[test]
fn severity_badges_before_triage() {
    for (name, severity) in [
        ("critical", Severity::Critical),
        ("warning", Severity::Warning),
        ("info", Severity::Info),
    ] {
        let mut card = card(CardBadge::Severity(severity), basic());
        card.next_step = None;
        insta::assert_json_snapshot!(format!("severity_{name}"), rendered(&card));
    }
}

#[test]
fn resolved_card_is_muted_with_no_buttons() {
    let mut card = card(CardBadge::Verdict(Verdict::Fixed), basic());
    card.resolved = true;
    insta::assert_json_snapshot!("resolved", rendered(&card));
}

#[test]
fn approval_buttons_for_read_and_write_actions() {
    let mut buttons = basic();
    buttons.extend([
        CardButton::Approve {
            execution: execution(1),
            token: token('a'),
            plan_prefix: None,
        },
        CardButton::Deny {
            execution: execution(1),
            token: token('a'),
        },
        CardButton::Approve {
            execution: execution(2),
            token: token('b'),
            plan_prefix: None,
        },
        CardButton::Deny {
            execution: execution(2),
            token: token('b'),
        },
    ]);
    let card = card(CardBadge::Verdict(Verdict::ActionNeeded), buttons);
    let value = rendered(&card);
    assert_eq!(
        value["components"].as_array().unwrap().len(),
        2,
        "5 per row"
    );
    insta::assert_json_snapshot!("approvals", value);
}

#[test]
fn destructive_action_links_to_the_web_ui() {
    let mut buttons = basic();
    buttons.push(CardButton::OpenInWeb {
        execution: execution(3),
    });
    let card = card(CardBadge::Verdict(Verdict::ActionNeeded), buttons);
    let value = rendered(&card);
    let link = &value["components"][0]["components"][3];
    assert_eq!(link["style"], 5, "a link button");
    assert!(
        link.get("custom_id").is_none(),
        "it can never be pressed into an approval"
    );
    insta::assert_json_snapshot!("destructive_link", value);
}

#[test]
fn no_buttons_and_long_titles() {
    let mut card = card(CardBadge::Verdict(Verdict::Monitor), Vec::new());
    card.headline = "x".repeat(400);
    card.bullets.clear();
    let value = rendered(&card);
    assert_eq!(
        value["embed"]["title"].as_str().unwrap().chars().count(),
        256
    );
    assert_eq!(value["components"], json!([]));
    insta::assert_json_snapshot!("no_buttons_long_title", value);
}

fn plan(execution: ExecutionId) -> CardPlan {
    CardPlan {
        execution,
        skill: "restart_container".to_owned(),
        host: HostName::parse("nas").unwrap(),
        risk: Risk::Write,
        args: vec![("container".to_owned(), "plex".to_owned())],
        plan_short: "0123abcd".to_owned(),
        expires_at: "2026-10-31T13:00:00Z".parse().unwrap(),
    }
}

#[test]
fn discord_card_names_target_and_args() {
    let mut buttons = basic();
    buttons.extend([
        CardButton::Approve {
            execution: execution(1),
            token: token('a'),
            plan_prefix: Some("0123ab".to_owned()),
        },
        CardButton::Deny {
            execution: execution(1),
            token: token('a'),
        },
    ]);
    let mut card = card(CardBadge::Verdict(Verdict::ActionNeeded), buttons);
    card.plans = vec![plan(execution(1))];
    let value = rendered(&card);

    let description = value["embed"]["description"].as_str().unwrap();
    assert!(description.contains("restart_container"), "{description}");
    assert!(description.contains("nas"), "{description}");
    assert!(description.contains("container=plex"), "{description}");
    assert!(description.contains("0123abcd"), "{description}");
    let heading = description.find("**Waiting for approval**").unwrap();
    let next = description.find("**Next:**").unwrap();
    assert!(heading < next, "the plan comes before the next step");

    let row = &value["components"][0]["components"];
    assert_eq!(row[3]["label"], "Approve restart_container on nas");
    assert_eq!(row[4]["label"], "Deny restart_container on nas");
    insta::assert_json_snapshot!("plan_and_approvals", value);

    // Discord allows 80 characters per label, so a long skill is cut and marked.
    card.plans[0].skill = "x".repeat(100);
    let value = rendered(&card);
    let label = value["components"][0]["components"][3]["label"]
        .as_str()
        .unwrap();
    assert_eq!(label.chars().count(), 80);
    assert!(label.ends_with('…'), "{label}");
}
