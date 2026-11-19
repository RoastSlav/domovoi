//! Chat buttons on the steps an investigation proposed: approve, deny and what is refused.

use domovoi_core::ports::{AuditFilter, AuditLog, InteractionSink, Page};
use domovoi_core::{
    ApprovalChannel, CardButton, ExecutionError, ExecutionId, InteractionKind, InteractionReply,
    OperationId, StepId,
};
use domovoi_engine::chat_token::{mint_token, token_hash};
use domovoi_engine::operations::OperationError;
use domovoi_hub::auth::interactions::{NOT_ALLOWED, step_reason};

use crate::api_harness::{Api, api_with_operations};
use crate::interactions::{ALLOWED, STRANGER, chat, press};

struct Waiting {
    operation: OperationId,
    step: StepId,
    token: String,
    prefix: String,
}

impl Waiting {
    fn action(&self) -> domovoi_core::ActionId {
        ExecutionId::from_step(self.step).as_action()
    }

    fn approve(&self) -> CardButton {
        CardButton::Approve {
            action: self.action(),
            token: self.token.clone(),
            plan_prefix: Some(self.prefix.clone()),
        }
    }
}

/// A proposed step of `skill` linked to the incident, holding a freshly rotated token.
async fn proposed_step(a: &Api, skill: &str, linked: bool) -> Waiting {
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

fn message(reply: &InteractionReply) -> &str {
    match reply {
        InteractionReply::Accepted { message } => message.as_deref().unwrap_or(""),
        InteractionReply::Rejected { reason } => reason,
    }
}

fn is_accepted(reply: &InteractionReply) -> bool {
    matches!(reply, InteractionReply::Accepted { .. })
}

async fn execution(a: &Api, w: &Waiting) -> domovoi_core::ExecutionRecord {
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

#[tokio::test]
async fn chat_approve_step_with_valid_token_authorizes_once() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "restart_container", true).await;

    let reply = c.authorizer.deliver(press(&a, ALLOWED, w.approve())).await;
    assert!(is_accepted(&reply), "{reply:?}");
    assert_eq!(message(&reply), "Approved restart_container");
    let stored = execution(&a, &w).await;
    let approval = stored.approval().expect("the step is approved");
    assert!(matches!(
        approval.approval().channel(),
        ApprovalChannel::Chat { platform_user, .. } if platform_user == ALLOWED
    ));
    let page = AuditLog::list(&*a.store, &AuditFilter::default(), &Page::first(100))
        .await
        .unwrap();
    assert!(page.items.iter().any(|e| e.kind == "chat.approve"));
    assert!(!format!("{:?}", page.items).contains(&w.token));

    let again = c.authorizer.deliver(press(&a, ALLOWED, w.approve())).await;
    assert_eq!(message(&again), "This button no longer works");
}

#[tokio::test]
async fn chat_approve_step_with_wrong_prefix_is_refused_and_keeps_the_token() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "restart_container", true).await;
    let mut wrong = w.approve();
    if let CardButton::Approve { plan_prefix, .. } = &mut wrong {
        *plan_prefix = Some("000000".to_owned());
    }
    let reply = c.authorizer.deliver(press(&a, ALLOWED, wrong)).await;
    assert_eq!(
        message(&reply),
        "This card is out of date; open the incident to review it"
    );
    assert!(execution(&a, &w).await.chat_token_hash().is_some());
    let ok = c.authorizer.deliver(press(&a, ALLOWED, w.approve())).await;
    assert!(is_accepted(&ok), "{ok:?}");
}

#[tokio::test]
async fn chat_approve_step_with_old_button_without_prefix_is_refused() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "restart_container", true).await;
    let mut old = w.approve();
    if let CardButton::Approve { plan_prefix, .. } = &mut old {
        *plan_prefix = None;
    }
    let reply = c.authorizer.deliver(press(&a, ALLOWED, old)).await;
    assert!(message(&reply).starts_with("This card is out of date"));
    assert!(execution(&a, &w).await.approval().is_none());
}

#[tokio::test]
async fn destructive_step_cannot_be_approved_from_discord() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "prune_images", true).await;
    let reply = c.authorizer.deliver(press(&a, ALLOWED, w.approve())).await;
    assert_eq!(message(&reply), "Approve this one in the web UI");
    assert!(execution(&a, &w).await.approval().is_none());
}

#[tokio::test]
async fn chat_deny_step_refuses_the_execution() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "restart_container", true).await;
    let deny = CardButton::Deny {
        action: w.action(),
        token: w.token.clone(),
    };
    let reply = c.authorizer.deliver(press(&a, ALLOWED, deny)).await;
    assert_eq!(message(&reply), "Denied restart_container");
    let snapshot = a
        .state
        .deps()
        .operation_repo
        .load(w.operation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.step(w.step).unwrap().progress.outcome,
        domovoi_core::StepOutcome::Refused
    );
    assert!(execution(&a, &w).await.chat_token_hash().is_none());
}

#[tokio::test]
async fn chat_press_on_a_step_of_another_incident_is_refused() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "restart_container", false).await;
    let reply = c.authorizer.deliver(press(&a, ALLOWED, w.approve())).await;
    assert_eq!(message(&reply), "That action no longer exists");
    assert!(execution(&a, &w).await.approval().is_none());
}

#[tokio::test]
async fn non_allowlisted_user_cannot_press_a_step_button() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "restart_container", true).await;
    let reply = c.authorizer.deliver(press(&a, STRANGER, w.approve())).await;
    assert_eq!(message(&reply), NOT_ALLOWED);
    assert!(execution(&a, &w).await.approval().is_none());
}

#[tokio::test]
async fn discord_message_text_can_never_approve_a_step() {
    let a = api_with_operations().await;
    a.admin().await;
    let c = chat(&a).await;
    let w = proposed_step(&a, "restart_container", true).await;
    let text = format!("approve {} {} {}", w.step, w.action(), w.token);
    let thread = InteractionKind::ThreadMessage {
        incident: a.incident.id(),
        text,
    };
    let _ = c
        .authorizer
        .deliver(crate::interactions::interaction(&a, ALLOWED, thread))
        .await;
    let stored = execution(&a, &w).await;
    assert!(stored.approval().is_none());
    assert!(stored.chat_token_hash().is_some());
}

#[tokio::test]
async fn step_reason_covers_every_operation_error() {
    let op = OperationId::new();
    let cases = [
        (OperationError::NotFound(op), "That action no longer exists"),
        (OperationError::BadToken, "This button no longer works"),
        (
            OperationError::Execution(ExecutionError::NeedsTypedConfirmation),
            "Approve this one in the web UI",
        ),
        (
            OperationError::Execution(ExecutionError::Expired),
            "The approval window has closed",
        ),
        (
            OperationError::Execution(ExecutionError::AlreadyAuthorized),
            "Someone already decided this action",
        ),
        (
            OperationError::Conflict,
            "This step was taken over; try again",
        ),
        (
            OperationError::Store("x".to_owned()),
            "Something went wrong; try again",
        ),
        (
            OperationError::Unavailable,
            "The service that decides actions is restarting; try again",
        ),
    ];
    for (err, text) in cases {
        assert_eq!(step_reason(&err), text);
    }
}
