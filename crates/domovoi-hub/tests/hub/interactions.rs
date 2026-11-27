//! The chat interaction authorizer over the API harness, with a recording gateway.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_core::notify::{ChatRefs, NotifyState};
use domovoi_core::ports::{
    AuditFilter, AuditLog, IncidentRepo, InteractionSink, MessageRef, NotifyStateRepo, Page,
    ThreadRef,
};
use domovoi_core::{
    CardButton, ExecutionId, IncidentStatus, Interaction, InteractionKind, InteractionReply,
    OperationId, Risk, StepId,
};
use domovoi_engine::chat_token::{mint_token, token_hash};
use domovoi_hub::auth::interactions::{ChatDeps, InteractionAuthorizer, NOT_ALLOWED, SETUP_FIRST};
use domovoi_testkit::{GatewayCall, RecordingGateway};
use jiff::SignedDuration;

use crate::api_harness::{Api, api, api_with_operations, api_with_triage, api_without_profiles};

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
        &a.services,
        ChatDeps {
            users: a.store.clone(),
            notify: a.store.clone(),
            audit: a.store.clone(),
            gateway: gateway.clone(),
            allowed: BTreeSet::from([ALLOWED.to_owned()]),
            incidents: a.store.clone(),
            hosts: a.store.clone(),
            notifier: a.state.deps().notifier.clone(),
            clock: a.clock.clone(),
            public_url: a.state.auth().web().public_url.clone(),
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

pub struct Waiting {
    pub operation: OperationId,
    pub step: StepId,
    pub token: String,
    pub prefix: String,
}

impl Waiting {
    pub fn execution_id(&self) -> ExecutionId {
        ExecutionId::from_step(self.step)
    }

    pub fn approve(&self) -> CardButton {
        CardButton::Approve {
            execution: self.execution_id(),
            token: self.token.clone(),
            plan_prefix: Some(self.prefix.clone()),
        }
    }
}

/// A proposed step of `skill` linked to the incident, holding a freshly rotated token.
pub async fn proposed_step(a: &Api, skill: &str, linked: bool) -> Waiting {
    let operation = a.create_operation("restart plex", linked).await;
    let args: &[(&str, &str)] = if skill == "restart_container" {
        &[("container", "plex")]
    } else {
        &[]
    };
    let step = a.propose_run(operation, skill, args).await;
    let token = mint_token();
    a.ops()
        .rotate_chat_token(operation, step, token_hash(&token))
        .await
        .unwrap();
    let snapshot = a.state.deps().operation_repo.load(operation).await.unwrap();
    let hash = snapshot
        .unwrap()
        .execution_of(step)
        .unwrap()
        .value
        .plan_hash;
    Waiting {
        operation,
        step,
        token,
        prefix: hex::encode(&hash.as_bytes()[..3]),
    }
}

pub async fn execution(a: &Api, w: &Waiting) -> domovoi_core::ExecutionRecord {
    let snapshot = a
        .state
        .deps()
        .operation_repo
        .load(w.operation)
        .await
        .unwrap()
        .unwrap();
    snapshot.execution_of(w.step).unwrap().value.clone()
}

/// A waiting `restart_container` (or `prune_images` when destructive) step linked to the
/// incident, with a chat token, as triage would leave it. Needs the operations harness.
pub async fn proposed(a: &Api, risk: Risk) -> Waiting {
    let skill = if risk == Risk::Destructive {
        "prune_images"
    } else {
        "restart_container"
    };
    proposed_step(a, skill, true).await
}

/// Whether the step has been approved.
pub async fn is_approved(a: &Api, w: &Waiting) -> bool {
    execution(a, w).await.approval().is_some()
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
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed(&a, Risk::Write).await;
    for kind in [
        InteractionKind::Button {
            button: CardButton::Ack,
            incident: a.incident.id(),
        },
        InteractionKind::Button {
            button: w.approve(),
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
    assert!(!is_approved(&a, &w).await);
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
async fn refused_presses_say_why_in_plain_words() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;
    let say = |button: CardButton| {
        let c = &c;
        let a = &a;
        async move {
            let reply = c.authorizer.deliver(press(a, ALLOWED, button)).await;
            match reply {
                InteractionReply::Rejected { reason } => reason,
                InteractionReply::Accepted { message } => message.unwrap_or_default(),
            }
        }
    };
    let plain = |text: &str| {
        for bad in [
            "Debug", "UserId", "Ack {", "Mute {", "Muted {", "{", "}", "event ", "01",
        ] {
            assert!(!text.contains(bad), "{text:?} leaks {bad:?}");
        }
    };

    assert_eq!(say(CardButton::Unmute).await, "This incident is not muted.");
    assert_eq!(say(CardButton::Ack).await, "Acknowledged");
    let again = say(CardButton::Ack).await;
    assert_eq!(again, "This incident is already acknowledged.");
    plain(&again);

    assert_eq!(say(CardButton::Mute1h).await, "Muted for an hour");
    let again = say(CardButton::Mute1h).await;
    assert!(again.starts_with("Already muted until "), "{again}");
    plain(&again);

    assert_eq!(say(CardButton::Unmute).await, "Unmuted");
    let incident = IncidentRepo::get(&*a.store, a.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incident.status(), IncidentStatus::Open);
}

#[tokio::test]
async fn details_posts_in_the_thread() {
    let a = api().await;
    a.admin().await;
    let c = chat(&a).await;

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

#[tokio::test]
async fn status_of_one_host_when_the_hub_collects_no_profiles() {
    let a = api_without_profiles().await;
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
    let named = c.authorizer.deliver(command("status", &["NAS"])).await;
    assert_eq!(
        accepted(&named),
        Some("• **nas**: offline, 1 open incident No profile yet for nas.")
    );
    let absent = c.authorizer.deliver(command("status", &["pantry"])).await;
    assert_eq!(rejected(&absent), "No host pantry; the hosts are nas");
}
