//! One real SQLite transaction per action transition: the action row, its audit entry and
//! its journal entry land together, or none of them does.

use std::collections::BTreeMap;

use domovoi_core::ports::ActionRepo;
use domovoi_core::{ActionRequest, ActionStatus, Principal, Risk, UserId};
use domovoi_engine::ApprovalError;
use domovoi_testkit::approvals::human;
use jiff::SignedDuration;

use crate::api_harness::api;

/// The status the store holds, as stored JSON.
async fn stored_status(
    store: &domovoi_store::Store,
    id: domovoi_core::ActionId,
) -> (String, Option<Vec<u8>>) {
    sqlx::query_as::<_, (String, Option<Vec<u8>>)>(
        "SELECT status, chat_token_hash FROM actions WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_one(store.pool_for_tests())
    .await
    .unwrap()
}

async fn audit_rows(store: &domovoi_store::Store) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap()
}

async fn journal_rows(store: &domovoi_store::Store) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM journal")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap()
}

#[tokio::test]
async fn audit_failure_rolls_back_transition_on_sqlite() {
    let a = api().await;
    let skill = domovoi_testkit::build::manifest("restart_container", Risk::Write)
        .unwrap()
        .summary();
    let args = skill.validate_args(&BTreeMap::new()).unwrap();
    let action = ActionRequest::propose(
        a.incident.id(),
        a.nas.id,
        &skill,
        &args,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        a.now(),
    )
    .with_chat_token_hash([0x11; 32]);
    let id = action.id();
    ActionRepo::insert(&*a.store, &action).await.unwrap();

    sqlx::query(
        "CREATE TRIGGER fail_audit BEFORE INSERT ON audit_log \
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .execute(a.store.pool_for_tests())
    .await
    .unwrap();

    let refused = a
        .actions_service
        .approve(id, human(UserId::new()), None)
        .await;
    assert!(
        matches!(refused, Err(ApprovalError::Store(_))),
        "an audit failure fails the transition: {refused:?}",
    );

    let (status, hash) = stored_status(&a.store, id).await;
    assert_eq!(
        status, r#"{"type":"proposed"}"#,
        "the action kept its previous status",
    );
    assert_eq!(hash, Some(vec![0x11; 32]), "the chat token hash is intact");
    assert_eq!(audit_rows(&a.store).await, 0, "no audit row");
    assert_eq!(journal_rows(&a.store).await, 0, "no journal row");

    sqlx::query("DROP TRIGGER fail_audit")
        .execute(a.store.pool_for_tests())
        .await
        .unwrap();

    let approved = a
        .actions_service
        .approve(id, human(UserId::new()), None)
        .await
        .unwrap();
    assert_eq!(approved.status(), &ActionStatus::Approved);
    assert_eq!(audit_rows(&a.store).await, 1, "one audit row");
    assert_eq!(journal_rows(&a.store).await, 1, "one journal row");
}
