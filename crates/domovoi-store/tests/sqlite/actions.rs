//! A guarded update never replaces a status the store already holds, and a status row
//! written before a field existed still loads.

use domovoi_core::ports::{ActionFilter, ActionRepo, Page, StoreError};
use domovoi_core::{ActionRequest, ActionStatus, HostId, IncidentId, Risk};
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

/// A row written when `Dispatched` carried no epoch still loads: the epoch is absent, not
/// invented, so reconciliation can ask its agent what became of the run. Its other fields
/// are untouched, and the `dispatched` filter still finds it.
#[tokio::test]
async fn pre_upgrade_dispatched_status_loads_without_an_epoch() {
    let fresh = Fresh::new().await;
    let open = action(IncidentId::new(), HostId::new(), Risk::Write).unwrap();
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
