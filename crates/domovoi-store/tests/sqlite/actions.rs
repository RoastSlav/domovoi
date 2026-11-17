//! A guarded update never replaces a status the store already holds, and a status row
//! written before a field existed still loads.

use domovoi_core::ports::{ActionFilter, ActionRepo, Page, StoreError};
use domovoi_core::{ActionRequest, ActionStatus, AuthorizationKind, HostId, IncidentId, Risk};
use domovoi_testkit::approvals::approve;
use domovoi_testkit::build::{T0, action};

use crate::common::Fresh;

#[tokio::test]
async fn update_from_refuses_a_stale_status() {
    let fresh = Fresh::new().await;
    let open = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    ActionRepo::insert(&fresh.store, &open).await.unwrap();

    // Someone decides the action before the writer gets back to the first copy.
    let mut decided = open.clone();
    decided.deny(T0).unwrap();
    ActionRepo::update_from(&fresh.store, &decided, "proposed")
        .await
        .unwrap();

    let refused = ActionRepo::update_from(&fresh.store, &open, "proposed").await;
    assert_eq!(
        refused,
        Err(StoreError::Conflict(
            "the action changed meanwhile".to_owned()
        )),
        "the stale copy is refused because the store has an outcome",
    );
    let stored = ActionRepo::get(&fresh.store, open.id()).await.unwrap();
    assert_eq!(
        stored.as_ref().map(ActionRequest::status),
        Some(&ActionStatus::Denied),
        "and nothing was written",
    );

    // The same write with the status the store holds is accepted.
    ActionRepo::update_from(&fresh.store, &decided, "denied")
        .await
        .unwrap();
    let kept = ActionRepo::get(&fresh.store, open.id()).await.unwrap();
    assert_eq!(
        kept.as_ref().map(ActionRequest::status),
        Some(&ActionStatus::Denied),
        "the decision is still the stored one",
    );

    // An id that was never stored is missing, not refused.
    let ghost = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    assert_eq!(
        ActionRepo::update_from(&fresh.store, &ghost, "proposed").await,
        Err(StoreError::NotFound),
        "a missing action is NotFound",
    );
}

#[tokio::test]
async fn legacy_actions_load_without_a_v2_binding() {
    let fresh = Fresh::new().await;
    let open = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    ActionRepo::insert(&fresh.store, &open).await.unwrap();
    sqlx::query(
        "UPDATE actions SET plan_hash_v2 = NULL, approved_plan_hash = NULL, \
         approval_session_ref = NULL, approval_client = NULL, authorization_kind = NULL \
         WHERE id = ?",
    )
    .bind(open.id().to_string())
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();

    let stored = ActionRepo::get(&fresh.store, open.id())
        .await
        .unwrap()
        .expect("a legacy row still loads");
    assert_eq!(stored.plan_hash_v2(), None, "no v2 binding is a v1 action");
    assert_eq!(
        stored.plan_hash_for_approval(),
        stored.plan_hash(),
        "the approval family falls back to v1",
    );
    assert_eq!(stored.approved_plan_hash(), None);
    assert_eq!(stored.authorization_kind(), None);
}

#[tokio::test]
async fn v2_binding_columns_round_trip() {
    let fresh = Fresh::new().await;
    let mut approved = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    approve(
        &mut approved,
        domovoi_testkit::approvals::human(domovoi_core::UserId::new()),
        T0,
    )
    .unwrap();
    ActionRepo::insert(&fresh.store, &approved).await.unwrap();

    let read = ActionRepo::get(&fresh.store, approved.id())
        .await
        .unwrap()
        .expect("the approved row loads");
    assert_eq!(
        read.plan_hash_v2(),
        approved.plan_hash_v2(),
        "the v2 hash survives"
    );
    assert_eq!(read.approved_plan_hash(), approved.approved_plan_hash());
    assert_eq!(
        read.authorization_kind(),
        Some(AuthorizationKind::Human),
        "the approval case is stored"
    );
    assert_eq!(read.provenance().map(|p| p.client()), Some("test"));

    ActionRepo::update_from(&fresh.store, &read, "approved")
        .await
        .unwrap();
    let again = ActionRepo::get(&fresh.store, approved.id())
        .await
        .unwrap()
        .expect("the updated row loads");
    assert_eq!(
        again, read,
        "every new column round-trips through an update"
    );
}

#[tokio::test]
async fn the_hash_columns_reject_a_non_hex_value() {
    let fresh = Fresh::new().await;
    let open = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    ActionRepo::insert(&fresh.store, &open).await.unwrap();

    for bad in ["a".repeat(63), "A".repeat(64)] {
        let refused = sqlx::query("UPDATE actions SET plan_hash_v2 = ? WHERE id = ?")
            .bind(bad)
            .bind(open.id().to_string())
            .execute(fresh.store.pool_for_tests())
            .await;
        assert!(refused.is_err(), "a non-canonical hash was accepted");
    }
}

async fn inserted(fresh: &Fresh) -> ActionRequest {
    let mut action = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    domovoi_testkit::approvals::approve(
        &mut action,
        domovoi_testkit::approvals::human(domovoi_core::UserId::new()),
        T0,
    )
    .unwrap();
    ActionRepo::insert(&fresh.store, &action).await.unwrap();
    action
}

async fn corrupts(fresh: &Fresh, id: ActionRequest, sql: &'static str) -> bool {
    sqlx::query(sql)
        .bind(id.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    matches!(
        ActionRepo::get(&fresh.store, id.id()).await,
        Err(StoreError::Backend(message)) if message.contains("unreadable")
    )
}

#[tokio::test]
async fn hydration_accepts_a_valid_proposed_v2_row() {
    let fresh = Fresh::new().await;
    let action = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    ActionRepo::insert(&fresh.store, &action).await.unwrap();
    assert!(
        ActionRepo::get(&fresh.store, action.id())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn hydration_accepts_a_valid_approved_v2_row() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    let stored = ActionRepo::get(&fresh.store, action.id())
        .await
        .unwrap()
        .expect("the approved row loads");
    assert_eq!(stored.authorization_kind(), Some(AuthorizationKind::Human));
}

#[tokio::test]
async fn hydration_rejects_a_v2_approval_without_a_displayed_hash() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    assert!(
        corrupts(
            &fresh,
            action,
            "UPDATE actions SET approved_plan_hash = NULL WHERE id = ?",
        )
        .await,
        "a v2 approval must keep the displayed hash",
    );
}

#[tokio::test]
async fn hydration_rejects_a_mismatched_approved_hash() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    let other = "11".repeat(32);
    let refused = sqlx::query("UPDATE actions SET approved_plan_hash = ? WHERE id = ?")
        .bind(other)
        .bind(action.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await;
    assert!(refused.is_ok(), "the CHECK accepts a canonical hash");
    assert!(
        matches!(
            ActionRepo::get(&fresh.store, action.id()).await,
            Err(StoreError::Backend(message)) if message.contains("does not match")
        ),
        "a hash from another plan is corrupt",
    );
}

#[tokio::test]
async fn hydration_rejects_an_approved_row_without_an_approver() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    assert!(
        corrupts(
            &fresh,
            action,
            "UPDATE actions SET approved_by = NULL, approved_channel = NULL, approved_at = NULL \
             WHERE id = ?",
        )
        .await,
        "a human approval needs its approver",
    );
}

#[tokio::test]
async fn hydration_rejects_approval_metadata_on_a_proposed_row() {
    let fresh = Fresh::new().await;
    let action = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    ActionRepo::insert(&fresh.store, &action).await.unwrap();
    let hash = "22".repeat(32);
    let updated = sqlx::query("UPDATE actions SET approved_plan_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(action.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await;
    assert!(updated.is_ok());
    assert!(
        matches!(
            ActionRepo::get(&fresh.store, action.id()).await,
            Err(StoreError::Backend(message)) if message.contains("undecided")
        ),
        "a proposed action must not carry approval metadata",
    );
}

#[tokio::test]
async fn hydration_accepts_a_v1_approval_with_a_matching_hash() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    sqlx::query(
        "UPDATE actions SET approved_plan_hash = plan_hash, plan_hash_v2 = NULL WHERE id = ?",
    )
    .bind(action.id().to_string())
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();
    let stored = ActionRepo::get(&fresh.store, action.id())
        .await
        .unwrap()
        .expect("the v1 approval loads");
    assert_eq!(
        stored.approval_plan().family(),
        domovoi_core::PlanFamily::V1
    );
}

#[tokio::test]
async fn hydration_accepts_a_legacy_approval_without_displayed_fields() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    sqlx::query(
        "UPDATE actions SET plan_hash_v2 = NULL, approved_plan_hash = NULL, \
         approval_session_ref = NULL, approval_client = NULL WHERE id = ?",
    )
    .bind(action.id().to_string())
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();
    let stored = ActionRepo::get(&fresh.store, action.id())
        .await
        .unwrap()
        .expect("the legacy approval loads");
    assert_eq!(stored.approved_plan_hash(), None);
    assert_eq!(stored.provenance(), None);
}

#[tokio::test]
async fn hydration_rejects_a_v1_approval_with_a_mismatched_hash() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    let other = "33".repeat(32);
    sqlx::query("UPDATE actions SET approved_plan_hash = ?, plan_hash_v2 = NULL WHERE id = ?")
        .bind(other)
        .bind(action.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert!(
        matches!(
            ActionRepo::get(&fresh.store, action.id()).await,
            Err(StoreError::Backend(message)) if message.contains("does not match")
        ),
        "a v1 displayed hash must match the v1 plan",
    );
}

/// A row written when `Dispatched` carried no epoch still loads: the epoch is absent, not
/// invented, so reconciliation can ask its agent what became of the run. Its other fields
/// are untouched, and the `dispatched` filter still finds it.
#[tokio::test]
async fn pre_upgrade_dispatched_status_loads_without_an_epoch() {
    let fresh = Fresh::new().await;
    let mut open = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
    domovoi_testkit::approvals::approve(
        &mut open,
        domovoi_testkit::approvals::human(domovoi_core::UserId::new()),
        T0,
    )
    .unwrap();
    ActionRepo::insert(&fresh.store, &open).await.unwrap();
    sqlx::query(r#"UPDATE actions SET status = '{"type":"dispatched"}' WHERE id = ?"#)
        .bind(open.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();

    let stored = ActionRepo::get(&fresh.store, open.id()).await.unwrap();
    let stored = stored.expect("the old row still loads");
    assert_eq!(
        stored.status(),
        &ActionStatus::Dispatched { agent_epoch: None },
        "the old row keeps its status and gains no epoch",
    );
    assert_eq!(stored.id(), open.id(), "and keeps its identity");
    assert_eq!(
        stored.plan_hash(),
        open.plan_hash(),
        "and the plan the approval bound to",
    );

    let filter = ActionFilter {
        status: Some("dispatched".to_owned()),
        ..ActionFilter::default()
    };
    let found = ActionRepo::list(&fresh.store, &filter, &Page::first(10))
        .await
        .unwrap();
    assert_eq!(
        found
            .items
            .iter()
            .map(ActionRequest::id)
            .collect::<Vec<_>>(),
        vec![open.id()],
        "the dispatched filter finds the old row",
    );
}

#[tokio::test]
async fn hydration_rejects_a_v1_approval_with_only_a_displayed_hash() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    sqlx::query(
        "UPDATE actions SET plan_hash_v2 = NULL, approved_plan_hash = plan_hash, \
         approval_session_ref = NULL, approval_client = NULL WHERE id = ?",
    )
    .bind(action.id().to_string())
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();
    assert!(
        matches!(
            ActionRepo::get(&fresh.store, action.id()).await,
            Err(StoreError::Backend(message)) if message.contains("provenance")
        ),
        "a current v1 approval must record where it was made",
    );
}

#[tokio::test]
async fn hydration_rejects_a_v1_approval_with_only_provenance() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    sqlx::query("UPDATE actions SET plan_hash_v2 = NULL, approved_plan_hash = NULL WHERE id = ?")
        .bind(action.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert!(
        matches!(
            ActionRepo::get(&fresh.store, action.id()).await,
            Err(StoreError::Backend(message)) if message.contains("displayed plan hash")
        ),
        "a current v1 approval must record the hash it approved",
    );
}

#[tokio::test]
async fn hydration_rejects_a_policy_row_that_claims_to_have_run() {
    let fresh = Fresh::new().await;
    let action = inserted(&fresh).await;
    sqlx::query(
        "UPDATE actions SET authorization_kind = 'policy', approved_by = NULL, \
         approved_channel = NULL, approved_at = NULL, approved_plan_hash = NULL, \
         approval_session_ref = NULL, approval_client = NULL, \
         status = '{\"type\":\"dispatched\"}' WHERE id = ?",
    )
    .bind(action.id().to_string())
    .execute(fresh.store.pool_for_tests())
    .await
    .unwrap();
    assert!(
        matches!(
            ActionRepo::get(&fresh.store, action.id()).await,
            Err(StoreError::Backend(message)) if message.contains("policy action is only accepted")
        ),
        "a policy row cannot claim to be a terminal execution at M1",
    );
}
