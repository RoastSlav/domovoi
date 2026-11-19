//! Action events of the old browser stream have an `action.transition` journal row naming the
//! same incident, written in the same commit as the status change.

use domovoi_core::ports::{AggregateRef, JournalEntry, JournalRead, Page};
use domovoi_core::{
    ActionId, ActionStatus, ApprovalProvenance, Authorization, PlanBinding, UserId,
};
use domovoi_engine::{ActionSettings, EventKind};
use domovoi_testkit::approvals::human;
use jiff::SignedDuration;

use crate::actions_harness::{Actions, actions};
use crate::harness::{host_id, restart_skill};

async fn approve(a: &Actions, id: ActionId) {
    let plan = a.get(id).await.plan_hash_for_approval();
    a.service
        .approve(
            id,
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(plan),
            ApprovalProvenance::new(None, "test"),
        )
        .await
        .unwrap();
}

async fn rows(a: &Actions, id: ActionId) -> Vec<JournalEntry> {
    JournalRead::entries_for(&*a.store, &AggregateRef::action(id), Page::MAX_LIMIT)
        .await
        .unwrap()
}

async fn propose(a: &Actions) -> ActionId {
    a.propose_for(
        &restart_skill(),
        &[("container", "nginx")],
        SignedDuration::from_hours(1),
    )
    .await
    .0
    .id()
}

#[tokio::test]
async fn action_changed_has_a_transition_row_naming_the_incident() {
    let mut a = actions(ActionSettings::default()).await;
    a.link.set_skills(host_id(), vec![restart_skill()]);
    let id = propose(&a).await;
    a.drain();
    approve(&a, id).await;
    a.until(id, |s| matches!(s, ActionStatus::Succeeded)).await;
    let published = a.drain();
    assert!(
        published
            .iter()
            .any(|e| matches!(e.kind, EventKind::ActionChanged { .. }))
    );
    let rows = rows(&a, id).await;
    assert!(rows.iter().all(|row| row.kind == "action.transition"));
    let to: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.payload["to"].as_str())
        .collect();
    assert!(
        to.contains(&"approved") && to.contains(&"succeeded"),
        "{to:?}"
    );
    for row in &rows {
        assert_eq!(row.payload["incident"], a.incident.id().to_string());
    }
}

#[tokio::test]
async fn action_blocked_has_a_rejected_transition_row_naming_the_incident() {
    let mut a = actions(ActionSettings::default()).await;
    a.link.set_skills(host_id(), vec![restart_skill()]);
    for _ in 0..3 {
        let id = propose(&a).await;
        approve(&a, id).await;
        a.until(id, |s| matches!(s, ActionStatus::Succeeded)).await;
    }
    let fourth = propose(&a).await;
    a.drain();
    approve(&a, fourth).await;
    a.until(fourth, |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    assert!(
        a.drain()
            .iter()
            .any(|e| matches!(e.kind, EventKind::ActionBlocked { .. }))
    );
    let rows = rows(&a, fourth).await;
    let last = rows.last().expect("the rejection is journaled");
    assert_eq!(last.kind, "action.transition");
    assert_eq!(last.payload["to"], "rejected");
    assert_eq!(last.payload["incident"], a.incident.id().to_string());
}
