//! A triage service built with its queue off starts no workers: investigations do the work.

use domovoi_engine::triage::TriageSettings;
use domovoi_engine::{DomainEvent, EventKind};
use tokio::task::yield_now;

use crate::harness::{Setup, report, world};

/// Gives the service's tasks turns to react to an event.
async fn settle() {
    for _ in 0..500 {
        yield_now().await;
    }
}

#[tokio::test]
async fn run_queue_false_starts_no_workers() {
    // With the queue on, the request is triaged: the control for what follows.
    let mut control = world(Setup::default()).await;
    control.primary.push_text(report());
    let event = DomainEvent::new(EventKind::TriageRequested, &control.incident, true);
    control.bus.send(event).unwrap();
    settle().await;
    assert_eq!(control.primary.requests().len(), 1, "the queue triaged it");
    assert!(!control.drain().is_empty());

    let mut w = world(Setup {
        settings: TriageSettings {
            run_queue: false,
            ..TriageSettings::default()
        },
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    let event = DomainEvent::new(EventKind::TriageRequested, &w.incident, true);
    w.bus.send(event).unwrap();
    settle().await;

    assert!(w.primary.requests().is_empty(), "no model call");
    assert_eq!(w.triage.queued(), 0);
    assert!(
        !w.drain()
            .iter()
            .any(|e| matches!(e.kind, EventKind::TriageCompleted { .. })),
        "nothing was triaged"
    );
    assert!(w.actions().await.is_empty());

    // The service's task is parked until shutdown, then ends.
    let task = w.triage_task.take().expect("the service's task");
    assert!(!task.is_finished());
    w.shutdown.cancel();
    task.await.unwrap();
}
