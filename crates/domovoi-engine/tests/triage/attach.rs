use std::collections::BTreeMap;

use domovoi_core::ports::IncidentRepo;
use domovoi_core::{IncidentStatus, TriageOutcome, TriageReport, Verdict};
use domovoi_engine::notify::{NotifyAction, NotifySettings, plan};
use domovoi_engine::{CommandError, DomainEvent, EventKind};
use domovoi_testkit::build::resolved_incident;
use jiff::tz::TimeZone;

use crate::harness::{Setup, host_id, report, world};

fn parsed() -> TriageReport {
    domovoi_engine::triage::parse_report(&report()).unwrap()
}

#[tokio::test]
async fn attach_triage_sets_verdict_and_emits_completed() {
    let mut w = world(Setup::default()).await;
    let outcome = TriageOutcome::Report(parsed());
    w.manager
        .attach_triage(w.incident.id(), outcome.clone(), BTreeMap::new())
        .await
        .unwrap();
    let stored = IncidentRepo::get(&*w.store, w.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.verdict(), Some(Verdict::ActionNeeded));
    assert_eq!(
        stored.status(),
        w.incident.status(),
        "triage never changes status"
    );
    let events = w.drain();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].kind,
        EventKind::TriageCompleted {
            outcome,
            tokens: BTreeMap::new()
        }
    );
    let timeline = IncidentRepo::timeline(&*w.store, w.incident.id())
        .await
        .unwrap();
    assert!(
        timeline
            .iter()
            .any(|e| e.kind == "triage.report" && e.summary == "nginx keeps restarting")
    );

    // A failed run keeps the verdict and still reaches the card.
    w.manager
        .attach_triage(
            w.incident.id(),
            TriageOutcome::Failed {
                reason: "down".to_owned(),
            },
            BTreeMap::new(),
        )
        .await
        .unwrap();
    let stored = IncidentRepo::get(&*w.store, w.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.verdict(), Some(Verdict::ActionNeeded));

    // Triage may finish after the incident resolved.
    let resolved = resolved_incident(host_id(), "disk.usage", Some("/srv"), w.now()).unwrap();
    IncidentRepo::insert(&*w.store, &resolved).await.unwrap();
    w.manager
        .attach_triage(
            resolved.id(),
            TriageOutcome::Report(parsed()),
            BTreeMap::new(),
        )
        .await
        .unwrap();
    let stored = IncidentRepo::get(&*w.store, resolved.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status(), IncidentStatus::Resolved);
    assert_eq!(stored.verdict(), Some(Verdict::ActionNeeded));

    let unknown = domovoi_core::IncidentId::new();
    assert_eq!(
        w.manager
            .attach_triage(unknown, TriageOutcome::Report(parsed()), BTreeMap::new())
            .await,
        Err(CommandError::UnknownIncident(unknown))
    );
}

#[tokio::test]
async fn triage_events_update_the_card_and_degraded_replies_in_the_thread() {
    let w = world(Setup::default()).await;
    let settings = NotifySettings::new(TimeZone::UTC);
    let now = w.now();
    let completed = DomainEvent::new(
        EventKind::TriageCompleted {
            outcome: TriageOutcome::Report(parsed()),
            tokens: BTreeMap::new(),
        },
        &w.incident,
        true,
    );
    let opened = plan(
        &DomainEvent::new(EventKind::IncidentOpened, &w.incident, true),
        &Default::default(),
        &settings,
        now,
    );
    let mut state = opened.state.clone();
    state.messages.insert(
        "discord".to_owned(),
        domovoi_core::ChatRefs {
            message: domovoi_core::ports::MessageRef {
                platform: "discord".to_owned(),
                id: "m1".to_owned(),
            },
            thread: None,
        },
    );
    assert_eq!(
        plan(&completed, &state, &settings, now).actions,
        vec![NotifyAction::UpdateCard {
            incident: w.incident.id()
        }]
    );
    let degraded = DomainEvent::new(
        EventKind::TriageDegraded {
            reason: "The last 3 triage runs failed".to_owned(),
        },
        &w.incident,
        true,
    );
    let actions = plan(&degraded, &state, &settings, now).actions;
    assert!(matches!(
        &actions[..],
        [NotifyAction::ThreadReply { text, .. }] if text.starts_with("Triage is degraded: The last 3")
    ));
}

#[tokio::test]
async fn triage_requested_event_is_queued_triaged_and_attached() {
    let mut w = world(Setup::default()).await;
    // Suppressed incidents (maintenance, ack rules) are not triaged; events are handled in
    // order, so the request after it shows it was skipped.
    w.bus
        .send(DomainEvent::new(
            EventKind::TriageRequested,
            &w.incident,
            false,
        ))
        .unwrap();
    w.primary.push_text(report());
    w.bus
        .send(DomainEvent::new(
            EventKind::TriageRequested,
            &w.incident,
            true,
        ))
        .unwrap();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let event = w.events.recv().await.unwrap();
            if let EventKind::TriageCompleted { outcome, .. } = event.kind {
                return outcome;
            }
        }
    })
    .await
    .expect("the queued request is triaged");
    assert!(matches!(completed, TriageOutcome::Report(_)));
    assert_eq!(
        w.primary.requests().len(),
        1,
        "only the notified request ran"
    );
    assert_eq!(w.triage.queued(), 0);
}
