use domovoi_core::{ActionError, ActionStatus, UserId};
use domovoi_engine::{ActionSettings, ApprovalError};
use domovoi_testkit::approvals::{chat, human};
use jiff::SignedDuration;

use crate::actions_harness::{actions, verified_restart};

#[tokio::test]
async fn chat_approval_requires_matching_single_use_token() {
    let a = actions(ActionSettings::default()).await;
    let (first, token) = a.propose().await;
    let (second, _) = a.propose().await;
    let discord = || chat(UserId::new(), "discord", "1234");

    for wrong in [None, Some("not-the-token"), Some(&*token.to_uppercase())] {
        assert_eq!(
            a.service.approve(first.id(), discord(), wrong).await.err(),
            Some(ApprovalError::BadToken)
        );
    }
    assert_eq!(
        a.service
            .approve(second.id(), discord(), Some(&token))
            .await
            .err(),
        Some(ApprovalError::BadToken),
        "a token only works for its own action"
    );
    assert_eq!(a.get(first.id()).await.status(), &ActionStatus::Proposed);

    let approved = a
        .service
        .approve(first.id(), discord(), Some(&token))
        .await
        .unwrap();
    assert!(matches!(approved.status(), ActionStatus::Approved));
    assert_eq!(
        a.get(first.id()).await.chat_token_hash(),
        None,
        "single use"
    );
    assert_eq!(
        a.service
            .approve(first.id(), discord(), Some(&token))
            .await
            .err(),
        Some(ApprovalError::BadToken)
    );
}

#[tokio::test]
async fn expired_action_cannot_be_approved() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    a.clock.advance(SignedDuration::from_mins(61));
    assert_eq!(
        a.service
            .approve(action.id(), human(UserId::new()), None)
            .await
            .err(),
        Some(ApprovalError::Refused(ActionError::Expired))
    );
    assert_eq!(a.get(action.id()).await.status(), &ActionStatus::Proposed);
    assert_eq!(a.runs(), 0);
}

#[tokio::test]
async fn expire_due_expires_and_audits() {
    let a = actions(ActionSettings::default()).await;
    let (short, _) = a
        .propose_for(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_mins(10),
        )
        .await;
    let (long, _) = a.propose().await;
    a.clock.advance(SignedDuration::from_mins(11));
    assert_eq!(a.service.expire_due().await.unwrap(), 1);
    assert_eq!(a.get(short.id()).await.status(), &ActionStatus::Expired);
    assert_eq!(a.get(short.id()).await.chat_token_hash(), None);
    assert_eq!(a.get(long.id()).await.status(), &ActionStatus::Proposed);
    assert_eq!(
        a.trail(short.id()).await,
        ["action.proposed", "action.expired"]
    );
    assert_eq!(a.service.expire_due().await.unwrap(), 0, "only once");
}

#[tokio::test]
async fn denied_action_is_audited_and_cannot_be_approved() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let denied = a
        .service
        .deny(action.id(), human(UserId::new()))
        .await
        .unwrap();
    assert_eq!(denied.status(), &ActionStatus::Denied);
    assert!(matches!(
        a.service
            .approve(action.id(), human(UserId::new()), None)
            .await,
        Err(ApprovalError::Refused(ActionError::NotAllowed { .. }))
    ));
    assert_eq!(
        a.trail(action.id()).await,
        ["action.proposed", "action.denied"]
    );
}
