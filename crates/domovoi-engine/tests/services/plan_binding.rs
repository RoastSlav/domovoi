//! An approval covers the plan the card showed: the request must name the action's
//! approval-family plan hash, checked after the chat token and before anything changes.

use domovoi_core::{ActionStatus, ApprovalProvenance, Authorization, PlanBinding, UserId};
use domovoi_engine::{ActionSettings, ApprovalError, EventKind};
use domovoi_testkit::approvals::{chat, human};
use jiff::SignedDuration;

use crate::actions_harness::{actions, verified_restart};

#[tokio::test]
async fn approve_with_stale_plan_hash_refused() {
    let mut a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let (other, _) = a
        .proposal(
            &verified_restart(),
            &[("container", "postgres")],
            SignedDuration::from_hours(1),
        )
        .await;

    let refused = a
        .service
        .approve(
            action.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(other.plan_hash_for_approval()),
            ApprovalProvenance::new(None, "test"),
        )
        .await;

    assert_eq!(refused.err(), Some(ApprovalError::PlanChanged));
    assert_eq!(a.get(action.id()).await.status(), &ActionStatus::Proposed);
    assert_eq!(a.trail(action.id()).await, ["action.proposed"]);
    assert!(
        !a.drain()
            .iter()
            .any(|event| matches!(event.kind, EventKind::ActionApproved { .. })),
        "a refused approval publishes nothing"
    );
}

#[tokio::test]
async fn approve_with_current_plan_hash_succeeds() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;

    let approved = a
        .service
        .approve(
            action.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(action.plan_hash_for_approval()),
            ApprovalProvenance::new(None, "test"),
        )
        .await
        .unwrap();

    assert_eq!(approved.status(), &ActionStatus::Approved);
    assert_ne!(
        a.get(action.id()).await.status(),
        &ActionStatus::Proposed,
        "the approval is stored"
    );
}

#[tokio::test]
async fn chat_approve_with_matching_prefix_succeeds_and_mismatching_prefix_keeps_the_token() {
    let a = actions(ActionSettings::default()).await;
    let (action, token) = a.propose().await;
    let hash = action.plan_hash_for_approval();
    let bytes = hash.as_bytes();
    let mut wrong = bytes[..3].to_vec();
    wrong[0] ^= 0xff;

    let refused = a
        .service
        .approve(
            action.id(),
            Authorization::human(chat(UserId::new(), "discord", "1234")),
            Some(&token),
            PlanBinding::Prefix(wrong),
            ApprovalProvenance::new(None, "test"),
        )
        .await;
    assert_eq!(refused.err(), Some(ApprovalError::PlanChanged));
    assert!(
        a.get(action.id()).await.chat_token_hash().is_some(),
        "a wrong prefix does not consume the token"
    );

    let approved = a
        .service
        .approve(
            action.id(),
            Authorization::human(chat(UserId::new(), "discord", "1234")),
            Some(&token),
            PlanBinding::Prefix(bytes[..3].to_vec()),
            ApprovalProvenance::new(None, "test"),
        )
        .await
        .unwrap();
    assert_eq!(approved.status(), &ActionStatus::Approved);
}

#[tokio::test]
async fn plan_check_runs_after_the_chat_token_check() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let mut wrong = action.plan_hash_for_approval().as_bytes()[..3].to_vec();
    wrong[0] ^= 0xff;

    let refused = a
        .service
        .approve(
            action.id(),
            Authorization::human(chat(UserId::new(), "discord", "1234")),
            Some("not-the-token"),
            PlanBinding::Prefix(wrong),
            ApprovalProvenance::new(None, "test"),
        )
        .await;

    assert_eq!(refused.err(), Some(ApprovalError::BadToken));
    assert_eq!(a.get(action.id()).await.status(), &ActionStatus::Proposed);
}
