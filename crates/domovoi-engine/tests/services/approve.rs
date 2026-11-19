use domovoi_core::{ActionError, ActionStatus, PlanBinding, UserId};
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
        let (authorization, provenance) = a.authorization(discord());
        assert_eq!(
            a.service
                .approve(
                    first.id(),
                    authorization,
                    wrong,
                    PlanBinding::Full(first.plan_hash_for_approval()),
                    provenance,
                )
                .await
                .err(),
            Some(ApprovalError::BadToken)
        );
    }
    let (authorization, provenance) = a.authorization(discord());
    assert_eq!(
        a.service
            .approve(
                second.id(),
                authorization,
                Some(&token),
                PlanBinding::Full(second.plan_hash_for_approval()),
                provenance,
            )
            .await
            .err(),
        Some(ApprovalError::BadToken),
        "a token only works for its own action"
    );
    assert_eq!(a.get(first.id()).await.status(), &ActionStatus::Proposed);

    let (authorization, provenance) = a.authorization(discord());
    let approved = a
        .service
        .approve(
            first.id(),
            authorization,
            Some(&token),
            PlanBinding::Full(first.plan_hash_for_approval()),
            provenance,
        )
        .await
        .unwrap();
    assert!(matches!(approved.status(), ActionStatus::Approved));
    assert_eq!(
        a.get(first.id()).await.chat_token_hash(),
        None,
        "single use"
    );
    let (authorization, provenance) = a.authorization(discord());
    assert_eq!(
        a.service
            .approve(
                first.id(),
                authorization,
                Some(&token),
                PlanBinding::Full(first.plan_hash_for_approval()),
                provenance,
            )
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
    let (authorization, provenance) = a.authorization(human(UserId::new()));
    assert_eq!(
        a.service
            .approve(
                action.id(),
                authorization,
                None,
                PlanBinding::Full(action.plan_hash_for_approval()),
                provenance,
            )
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
    let (authorization, provenance) = a.authorization(human(UserId::new()));
    assert!(matches!(
        a.service
            .approve(
                action.id(),
                authorization,
                None,
                PlanBinding::Full(action.plan_hash_for_approval()),
                provenance,
            )
            .await,
        Err(ApprovalError::Refused(ActionError::NotAllowed { .. }))
    ));
    assert_eq!(
        a.trail(action.id()).await,
        ["action.proposed", "action.denied"]
    );
}

#[tokio::test]
async fn rotating_a_legacy_chat_token_retires_the_old_one_and_keeps_the_window() {
    let a = actions(ActionSettings::default()).await;
    let (action, old) = a.propose().await;
    let new = domovoi_engine::chat_token::mint_token();

    let rotated = a
        .service
        .rotate_chat_token(action.id(), domovoi_engine::chat_token::token_hash(&new))
        .await
        .unwrap();
    assert_eq!(
        rotated.expires_at(),
        action.expires_at(),
        "no longer window"
    );

    let discord = || chat(UserId::new(), "discord", "1234");
    let (authorization, provenance) = a.authorization(discord());
    assert_eq!(
        a.service
            .approve(
                action.id(),
                authorization,
                Some(&old),
                PlanBinding::Full(action.plan_hash_for_approval()),
                provenance,
            )
            .await
            .err(),
        Some(ApprovalError::BadToken),
        "the replaced token no longer works"
    );
    let (authorization, provenance) = a.authorization(discord());
    let approved = a
        .service
        .approve(
            action.id(),
            authorization,
            Some(&new),
            PlanBinding::Full(action.plan_hash_for_approval()),
            provenance,
        )
        .await
        .unwrap();
    assert!(matches!(approved.status(), ActionStatus::Approved));

    let (late, _) = a.propose().await;
    a.clock.advance(SignedDuration::from_hours(2));
    assert_eq!(
        a.service.rotate_chat_token(late.id(), [9; 32]).await.err(),
        Some(ApprovalError::Refused(ActionError::Expired)),
        "an expired window is not rotated"
    );
}
