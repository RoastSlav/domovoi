//! Every event the old browser stream sent has a journal row naming the same incident, and a
//! fold writes its own row in the same commit.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    AggregateRef, IncidentFilter, IncidentRepo, JournalEntry, JournalRead, Page,
};
use domovoi_core::{
    HostId, Incident, IncidentId, Severity, TriageOutcome, TriageReport, UserId, Verdict,
};
use domovoi_engine::{
    DomainEvent, EnginePolicy, EventKind, IncidentManager, ManagerDeps, ManagerHandle,
    PatternRedactor,
};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::SignedDuration;
use serde_json::json;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::harness::Sig;

struct Run {
    handle: ManagerHandle,
    events: broadcast::Receiver<DomainEvent>,
    store: Arc<InMemoryStore>,
    clock: Arc<FakeClock>,
    shutdown: CancellationToken,
}

fn start() -> Run {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let (bus, events) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let deps = ManagerDeps {
        incidents: store.clone(),
        uow: store.clone(),
        versions: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy: EnginePolicy::default(),
        bus,
        shutdown: shutdown.clone(),
    };
    let (handle, _task) = IncidentManager::spawn(deps);
    Run {
        handle,
        events,
        store,
        clock,
        shutdown,
    }
}

impl Run {
    fn drain(&mut self) -> Vec<DomainEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            out.push(event);
        }
        out
    }

    async fn head(&self) -> u64 {
        self.store.head_seq().await.unwrap()
    }

    async fn rows_since(&self, head: u64) -> Vec<JournalEntry> {
        self.store.entries_after(head, 500).await.unwrap()
    }

    async fn incident(&self) -> Incident {
        IncidentRepo::list(&*self.store, &IncidentFilter::default(), &Page::first(100))
            .await
            .unwrap()
            .items
            .remove(0)
    }
}

fn payload_kinds(rows: &[JournalEntry]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.kind == "incident.timeline")
        .map(|row| row.payload["kind"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn report() -> TriageReport {
    TriageReport {
        headline: "h".to_owned(),
        summary: "s".to_owned(),
        cause: "c".to_owned(),
        evidence: Vec::new(),
        verdict: Verdict::ActionNeeded,
        next_step: "n".to_owned(),
        manual_steps: Vec::new(),
        proposals: Vec::new(),
        details: String::new(),
    }
}

/// What one scenario must have produced: the published event and the journal kinds.
struct Expect<'a> {
    name: &'a str,
    event: fn(&EventKind) -> bool,
    timeline_kinds: &'a [&'a str],
    extra_kind: Option<&'a str>,
}

async fn check(run: &mut Run, head: u64, incident: IncidentId, want: Expect<'_>) {
    let published = run.drain();
    let rows = run.rows_since(head).await;
    let name = want.name;
    assert!(
        published.iter().any(|e| (want.event)(&e.kind)),
        "{name}: no event published: {published:?}"
    );
    let own = AggregateRef::incident(incident);
    assert!(
        rows.iter().any(|row| row.aggregate == own),
        "{name}: no journal row names the incident"
    );
    let found = payload_kinds(&rows);
    for kind in want.timeline_kinds {
        assert!(
            found.iter().any(|k| k == kind),
            "{name}: missing {kind} in {found:?}"
        );
    }
    if let Some(extra) = want.extra_kind {
        assert!(
            rows.iter().any(|row| row.kind == extra),
            "{name}: missing {extra}"
        );
    }
}

#[tokio::test]
async fn every_v1_event_has_a_journal_equivalent() {
    let mut run = start();
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    let user = UserId::new();

    let head = run.head().await;
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    let id = run.incident().await.id();
    let want = Expect {
        name: "opened",
        event: |k| matches!(k, EventKind::IncidentOpened),
        timeline_kinds: &["incident.created", "status.changed"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    run.handle.ingest(vec![disk.firing(1)]).await.unwrap();
    let want = Expect {
        name: "folded",
        event: |k| matches!(k, EventKind::IncidentFolded { .. }),
        timeline_kinds: &[],
        extra_kind: Some("incident.folded"),
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    let critical = disk.clone().severity(Severity::Critical);
    run.handle.ingest(vec![critical.firing(2)]).await.unwrap();
    let want = Expect {
        name: "escalated",
        event: |k| matches!(k, EventKind::IncidentEscalated { .. }),
        timeline_kinds: &["severity.changed"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    run.handle.ack(id, user).await.unwrap();
    let want = Expect {
        name: "acked",
        event: |k| matches!(k, EventKind::IncidentAcked { .. }),
        timeline_kinds: &["human.action"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    let until = T0 + SignedDuration::from_secs(3600);
    run.handle.mute(id, until, user).await.unwrap();
    let want = Expect {
        name: "muted",
        event: |k| matches!(k, EventKind::IncidentMuted { .. }),
        timeline_kinds: &["human.action", "status.changed"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    run.handle.unmute(id).await.unwrap();
    let want = Expect {
        name: "unmuted",
        event: |k| matches!(k, EventKind::IncidentUnmuted),
        timeline_kinds: &["status.changed"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    run.handle
        .attach_triage(id, TriageOutcome::Report(report()), BTreeMap::new())
        .await
        .unwrap();
    let want = Expect {
        name: "triage",
        event: |k| matches!(k, EventKind::TriageCompleted { .. }),
        timeline_kinds: &["triage.report"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    run.handle.ingest(vec![disk.resolved(10)]).await.unwrap();
    let want = Expect {
        name: "resolved",
        event: |k| matches!(k, EventKind::IncidentResolved),
        timeline_kinds: &["status.changed"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;

    let head = run.head().await;
    run.clock.advance(SignedDuration::from_secs(20));
    run.handle.ingest(vec![disk.firing(20)]).await.unwrap();
    let want = Expect {
        name: "reopened",
        event: |k| matches!(k, EventKind::IncidentReopened),
        timeline_kinds: &["status.changed"],
        extra_kind: None,
    };
    check(&mut run, head, id, want).await;
    run.shutdown.cancel();
}

#[tokio::test]
async fn fold_writes_an_incident_folded_row_in_the_same_commit() {
    let mut run = start();
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    let id = run.incident().await.id();
    run.drain();
    let second = disk.firing(1);
    let head = run.head().await;
    run.handle.ingest(vec![second.clone()]).await.unwrap();
    let published = run.drain();
    let occurrences = published
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::IncidentFolded { occurrences, .. } => Some(*occurrences),
            _ => None,
        })
        .expect("a fold was published");
    let folded: Vec<_> = run
        .rows_since(head)
        .await
        .into_iter()
        .filter(|row| row.kind == "incident.folded")
        .collect();
    assert_eq!(folded.len(), 1);
    assert_eq!(folded[0].aggregate, AggregateRef::incident(id));
    assert_eq!(folded[0].actor, "system:incidents");
    assert_eq!(
        folded[0].payload,
        json!({"signal": second.id().to_string(), "occurrences": occurrences})
    );
    run.shutdown.cancel();
}

#[tokio::test]
async fn a_signal_that_does_not_fold_writes_no_folded_row() {
    let run = start();
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    run.handle.ingest(vec![disk.resolved(0)]).await.unwrap();
    let other = Sig::new(HostId::new(), "power.supply", None).immediate();
    run.handle.ingest(vec![other.firing(1)]).await.unwrap();
    let rows = run.rows_since(0).await;
    assert!(rows.iter().all(|row| row.kind != "incident.folded"));
    run.shutdown.cancel();
}
