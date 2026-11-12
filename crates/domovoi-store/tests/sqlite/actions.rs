//! A guarded update never replaces a status the store already holds.

use domovoi_core::ports::{ActionRepo, StoreError};
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
