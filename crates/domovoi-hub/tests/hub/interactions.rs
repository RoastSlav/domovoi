//! The chat interaction authorizer over the API harness, with a recording gateway.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_core::notify::{ChatRefs, NotifyState};
use domovoi_core::ports::{
    ActionRepo, AuditFilter, AuditLog, IncidentRepo, InteractionSink, MessageRef, NotifyStateRepo,
    Page, ThreadRef,
};
use domovoi_core::{
    ActionRequest, ActionStatus, CardButton, IncidentStatus, Interaction, InteractionKind,
    InteractionReply, Principal, Risk,
};
use domovoi_engine::triage::{mint_token, token_hash};
use domovoi_hub::auth::interactions::{ChatDeps, InteractionAuthorizer, NOT_ALLOWED, SETUP_FIRST};
use domovoi_testkit::build::manifest;
use domovoi_testkit::{GatewayCall, RecordingGateway};
use jiff::SignedDuration;

use crate::api_harness::{Api, api, api_with_triage};

/// The allowlisted Discord user.
pub const ALLOWED: &str = "234567890123456789";

/// Someone else in the server.
pub const STRANGER: &str = "999999999999999999";

pub struct Chat {
    pub authorizer: InteractionAuthorizer,
    pub gateway: Arc<RecordingGateway>,
    pub thread: ThreadRef,
}

/// An authorizer for `a` with the incident's card and thread recorded, as the notifier
/// would have left them.
pub async fn chat(a: &Api) -> Chat {
    let gateway = Arc::new(RecordingGateway::new("discord"));
    let thread = ThreadRef {
        platform: "discord".to_owned(),
        id: "1111".to_owned(),
    };
    let state = NotifyState {
        messages: BTreeMap::from([(
            "discord".to_owned(),
            ChatRefs {
                message: MessageRef {
                    platform: "discord".to_owned(),
                    id: "1111".to_owned(),
                },
                thread: Some(thread.clone()),
            },
        )]),
        ..NotifyState::default()
    };
    NotifyStateRepo::put(&*a.store, a.incident.id(), &state, a.now())
        .await
        .unwrap();
    let authorizer = InteractionAuthorizer::new(
        &a.state,
        ChatDeps {
            users: a.store.clone(),
            notify: a.store.clone(),
            audit: a.store.clone(),
            gateway: gateway.clone(),
            allowed: BTreeSet::from([ALLOWED.to_owned()]),
        },
    );
    Chat {
        authorizer,
        gateway,
        thread,
    }
}

pub fn interaction(a: &Api, user: &str, kind: InteractionKind) -> Interaction {
    Interaction {
        platform: "discord".to_owned(),
        platform_user: user.to_owned(),
        kind,
        received_at: a.now(),
    }
}

pub fn press(a: &Api, user: &str, button: CardButton) -> Interaction {
    interaction(
        a,
        user,
        InteractionKind::Button {
            button,
            incident: a.incident.id(),
        },
    )
}

/// A proposed `restart_container` (or `prune_snapshots` when destructive) with a chat
/// token, as triage would leave it.
pub async fn proposed(a: &Api, risk: Risk) -> (ActionRequest, String) {
    let name = if risk == Risk::Destructive {
        "prune_snapshots"
    } else {
        "restart_container"
    };
    let summary = manifest(name, risk).unwrap().summary();
    let args = summary.validate_args(&BTreeMap::new()).unwrap();
    let token = mint_token();
    let action = ActionRequest::propose(
        a.incident.id(),
        a.nas.id,
        &summary,
        &args,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        a.now(),
    )
    .with_chat_token_hash(token_hash(&token));
    ActionRepo::insert(&*a.store, &action).await.unwrap();
    (action, token)
}

pub async fn status_of(a: &Api, action: &ActionRequest) -> ActionStatus {
    ActionRepo::get(&*a.store, action.id())
        .await
        .unwrap()
        .unwrap()
        .status()
        .clone()
}

pub fn approve(action: &ActionRequest, token: &str) -> CardButton {
    CardButton::Approve {
        action: action.id(),
        token: token.to_owned(),
    }
}

fn accepted(reply: &InteractionReply) -> Option<&str> {
    match reply {
        InteractionReply::Accepted { message } => Some(message.as_deref().unwrap_or("")),
        InteractionReply::Rejected { .. } => None,
    }
}

fn rejected(reply: &InteractionReply) -> &str {
    match reply {
        InteractionReply::Rejected { reason } => reason,
        InteractionReply::Accepted { .. } => panic!("expected a rejection, got {reply:?}"),
    }
}

async fn audit_kinds(a: &Api) -> Vec<(String, String)> {
    let page = AuditLog::list(&*a.store, &AuditFilter::default(), &Page::first(100))
        .await
        .unwrap();
    page.items
        .into_iter()
        .filter(|e| e.kind.starts_with("chat."))
        .map(|e| (e.kind, e.actor))
        .collect()
}

#[tokio::test]
async fn non_allowlisted_user_is_rejected() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;
    let (action, token) = proposed(&a, Risk::Write).await;
    for kind in [
        InteractionKind::Button {
            button: CardButton::Ack,
            incident: a.incident.id(),
        },
        InteractionKind::Button {
            button: approve(&action, &token),
            incident: a.incident.id(),
        },
        InteractionKind::ThreadMessage {
            incident: a.incident.id(),
            text: "what happened?".to_owned(),
        },
        InteractionKind::Command {
            name: "status".to_owned(),
            args: Vec::new(),
        },
    ] {
        let reply = c.authorizer.deliver(interaction(&a, STRANGER, kind)).await;
        assert_eq!(rejected(&reply), NOT_ALLOWED);
    }
    assert_eq!(status_of(&a, &action).await, ActionStatus::Proposed);
    assert_eq!(
        IncidentRepo::get(&*a.store, a.incident.id())
            .await
            .unwrap()
            .unwrap()
            .status(),
        IncidentStatus::Open
    );
    assert!(c.gateway.calls().is_empty());
    assert!(audit_kinds(&a).await.is_empty(), "nothing was done");
}

#[tokio::test]
async fn chat_does_nothing_before_setup() {
    let a = api().await;
    let c = chat(&a).await;
    let reply = c
        .authorizer
        .deliver(press(&a, ALLOWED, CardButton::Ack))
        .await;
    assert_eq!(rejected(&reply), SETUP_FIRST);
}

#[tokio::test]
async fn ack_and_mute_reach_incident_manager() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;

    let reply = c
        .authorizer
        .deliver(press(&a, ALLOWED, CardButton::Ack))
        .await;
    assert_eq!(accepted(&reply), Some("Acknowledged"));
    let incident = IncidentRepo::get(&*a.store, a.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(incident.status(), IncidentStatus::Acked),
        "{:?}",
        incident.status()
    );

    let reply = c
        .authorizer
        .deliver(press(&a, ALLOWED, CardButton::Mute1h))
        .await;
    assert_eq!(accepted(&reply), Some("Muted for an hour"));
    let incident = IncidentRepo::get(&*a.store, a.incident.id())
        .await
        .unwrap()
        .unwrap();
    let IncidentStatus::Muted { until } = incident.status() else {
        panic!("muted, not {:?}", incident.status());
    };
    assert_eq!(
        until,
        a.now().checked_add(SignedDuration::from_hours(1)).unwrap()
    );

    assert_eq!(
        audit_kinds(&a).await,
        [
            ("chat.mute".to_owned(), format!("chat:discord:{ALLOWED}")),
            ("chat.ack".to_owned(), format!("chat:discord:{ALLOWED}")),
        ]
    );
}

#[tokio::test]
async fn approve_with_valid_token_approves_once() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;
    let (action, token) = proposed(&a, Risk::Write).await;

    let reply = c
        .authorizer
        .deliver(press(&a, ALLOWED, approve(&action, &token)))
        .await;
    assert_eq!(accepted(&reply), Some("Approved restart_container"));
    let stored = ActionRepo::get(&*a.store, action.id())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(stored.status(), &ActionStatus::Proposed);
    let approval = serde_json::to_value(stored.approved_by().unwrap()).unwrap();
    assert_eq!(approval["channel"]["type"], "chat");
    assert_eq!(approval["channel"]["platform_user"], ALLOWED);
    assert!(
        audit_kinds(&a)
            .await
            .contains(&("chat.approve".to_owned(), format!("chat:discord:{ALLOWED}")))
    );
}

#[tokio::test]
async fn reused_token_is_rejected() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;
    let (action, token) = proposed(&a, Risk::Write).await;
    let (other, _) = proposed(&a, Risk::Write).await;

    let wrong = c
        .authorizer
        .deliver(press(&a, ALLOWED, approve(&other, &token)))
        .await;
    assert_eq!(
        rejected(&wrong),
        "This button no longer works",
        "another action's token"
    );
    assert_eq!(status_of(&a, &other).await, ActionStatus::Proposed);

    let first = c
        .authorizer
        .deliver(press(&a, ALLOWED, approve(&action, &token)))
        .await;
    assert!(accepted(&first).is_some());
    let again = c
        .authorizer
        .deliver(press(&a, ALLOWED, approve(&action, &token)))
        .await;
    assert!(
        accepted(&again).is_none(),
        "the token works once: {again:?}"
    );
}

#[tokio::test]
async fn deny_and_details() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;
    let (action, token) = proposed(&a, Risk::Write).await;
    let deny = CardButton::Deny {
        action: action.id(),
        token,
    };
    let reply = c.authorizer.deliver(press(&a, ALLOWED, deny)).await;
    assert_eq!(accepted(&reply), Some("Denied restart_container"));
    assert_eq!(status_of(&a, &action).await, ActionStatus::Denied);

    let reply = c
        .authorizer
        .deliver(press(&a, ALLOWED, CardButton::Details))
        .await;
    assert_eq!(accepted(&reply), Some("Posted in the thread"));
    assert_eq!(
        c.gateway.calls(),
        [GatewayCall::Reply(
            c.thread.clone(),
            "No triage report yet.".to_owned()
        )]
    );
}

#[tokio::test]
async fn thread_message_becomes_follow_up_and_reply() {
    let a = api_with_triage().await;
    a.admin().await;
    let c = chat(&a).await;
    a.llm
        .push_text("The nightly backup filled /srv; prune old snapshots.");

    let ask = InteractionKind::ThreadMessage {
        incident: a.incident.id(),
        text: "Why is the disk filling up?".to_owned(),
    };
    let reply = c.authorizer.deliver(interaction(&a, ALLOWED, ask)).await;
    assert_eq!(
        accepted(&reply),
        Some(""),
        "the answer goes in the thread instead"
    );
    assert_eq!(
        c.gateway.calls(),
        [GatewayCall::Reply(
            c.thread.clone(),
            "The nightly backup filled /srv; prune old snapshots.".to_owned()
        )]
    );
    let asked = a.llm.requests();
    assert_eq!(asked.len(), 1);
    assert!(format!("{:?}", asked[0]).contains("Why is the disk filling up?"));
    assert!(audit_kinds(&a).await.contains(&(
        "chat.follow_up".to_owned(),
        format!("chat:discord:{ALLOWED}")
    )));
}

#[tokio::test]
async fn commands_status_incidents_and_mute_by_short_id() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;
    let command = |name: &str, args: &[&str]| {
        interaction(
            &a,
            ALLOWED,
            InteractionKind::Command {
                name: name.to_owned(),
                args: args.iter().map(|s| (*s).to_owned()).collect(),
            },
        )
    };
    let status = c.authorizer.deliver(command("status", &[])).await;
    assert_eq!(
        accepted(&status),
        Some("• **nas**: offline, 1 open incident")
    );

    let short = domovoi_chat::discord::ids::short_id(a.incident.id());
    let list = c.authorizer.deliver(command("incidents", &[])).await;
    let text = accepted(&list).unwrap();
    assert!(text.contains(&format!("`{short}`")), "{text}");
    assert!(
        text.contains(&format!("/incidents/{}", a.incident.id())),
        "{text}"
    );

    let bad = c.authorizer.deliver(command("mute", &[&short, "2d"])).await;
    assert_eq!(rejected(&bad), "The duration must be 1h, 4h or 24h");
    let unknown = c
        .authorizer
        .deliver(command("mute", &["deadbeef", "1h"]))
        .await;
    assert_eq!(rejected(&unknown), "No open incident deadbeef");
    let muted = c.authorizer.deliver(command("mute", &[&short, "4h"])).await;
    assert_eq!(
        accepted(&muted),
        Some(format!("Muted {short} for 4h").as_str())
    );
    let incident = IncidentRepo::get(&*a.store, a.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(incident.status(), IncidentStatus::Muted { .. }));

    let digest = c.authorizer.deliver(command("digest", &[])).await;
    assert_eq!(
        rejected(&digest),
        "No chat is configured to send the digest to"
    );
    let nope = c.authorizer.deliver(command("reboot", &[])).await;
    assert_eq!(rejected(&nope), "Unknown command /reboot");
}
