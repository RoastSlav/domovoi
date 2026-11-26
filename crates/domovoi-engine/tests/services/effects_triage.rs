//! Triage across a restart: an incident left open without a report is queued again, while
//! one a person or a rule already handled waits for the lag-triggered reload.

use std::sync::Arc;

use domovoi_core::incident::{IncidentEvent, IncidentPolicy, apply};
use domovoi_core::ports::{
    IncidentRepo, IncidentTimelineEntry, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{Incident, IncidentId, UserId};
use domovoi_engine::{DomainEvent, EventKind};
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::{T0, open_incident};

use crate::harness::{World, host_id, report, world_over};

/// How many `triage.` entries an incident has.
async fn triage_entries(store: &InMemoryStore, id: IncidentId) -> Vec<String> {
    IncidentRepo::timeline(store, id)
        .await
        .expect("the timeline is readable")
        .into_iter()
        .map(|entry| entry.kind)
        .filter(|kind| kind.starts_with("triage."))
        .collect()
}

fn acked_incident(check: &str) -> Incident {
    let mut incident = open_incident(host_id(), check, None).expect("a valid incident");
    apply(
        &mut incident,
        IncidentEvent::Ack { by: UserId::new() },
        &IncidentPolicy::default(),
        T0,
    )
    .expect("an open incident can be acknowledged");
    incident
}

/// A store holding three active incidents the previous session left behind: one open and
/// never triaged, one acknowledged by a person, one already triaged.
async fn seeded_store() -> (Arc<InMemoryStore>, IncidentId, IncidentId, IncidentId) {
    let store = Arc::new(InMemoryStore::new());
    let open = open_incident(host_id(), "disk.space", Some("/srv")).expect("a valid incident");
    IncidentRepo::insert(&*store, &open)
        .await
        .expect("the open incident is stored");
    let acked = acked_incident("net.link");
    IncidentRepo::insert(&*store, &acked)
        .await
        .expect("the acknowledged incident is stored");
    let triaged = open_incident(host_id(), "load.high", None).expect("a valid incident");
    IncidentRepo::insert(&*store, &triaged)
        .await
        .expect("the triaged incident is stored");
    let entry = IncidentTimelineEntry {
        incident: triaged.id(),
        at: T0,
        kind: "triage.report".to_owned(),
        summary: "the previous session triaged it".to_owned(),
        detail: serde_json::Value::Null,
    };
    UnitOfWork::commit(
        &*store,
        Transaction::new().write(StateWrite::IncidentEvent(entry)),
    )
    .await
    .expect("the report is stored");
    (store, open.id(), acked.id(), triaged.id())
}

/// Gives the model enough reports for every incident the service may triage.
fn script(world: &World, count: usize) {
    for _ in 0..count {
        world.primary.push_text(report());
    }
}

/// Lets the service triage what the start-up pass queued.
async fn settle() {
    for _ in 0..500 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn untriaged_incident_triaged_after_restart() {
    let (store, open, acked, triaged) = seeded_store().await;
    let world = world_over(Default::default(), store.clone()).await;
    script(&world, 4);
    settle().await;

    assert!(
        !triage_entries(&store, open).await.is_empty(),
        "an incident left open without a report is triaged after the restart"
    );
    assert!(
        triage_entries(&store, acked).await.is_empty(),
        "an incident a person acknowledged is not triaged on their behalf"
    );
    assert_eq!(
        triage_entries(&store, triaged).await,
        ["triage.report"],
        "the incident the previous session triaged is left alone"
    );
}

#[tokio::test]
async fn lag_reload_still_queues_acked_incidents() {
    let (store, _open, acked, _triaged) = seeded_store().await;
    let world = world_over(Default::default(), store.clone()).await;
    script(&world, 4);
    settle().await;
    assert!(
        triage_entries(&store, acked).await.is_empty(),
        "the start-up pass leaves an acknowledged incident alone"
    );

    // Overflow the bus while the service cannot run, so its next read reports a lag.
    for _ in 0..300 {
        let _ = world.bus.send(DomainEvent::new(
            EventKind::IncidentUnmuted,
            &world.incident,
            true,
        ));
    }
    settle().await;

    assert!(
        !triage_entries(&store, acked).await.is_empty(),
        "the lag-triggered reload still queues an acknowledged incident"
    );
}
