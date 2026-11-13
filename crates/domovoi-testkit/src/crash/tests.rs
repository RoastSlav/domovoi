//! Unit tests for the crash wrapper, snapshots and invariants.

use std::sync::Arc;

use domovoi_core::ports::StoreError;
use domovoi_core::ports::{
    ActionGuard, AggregateRef, AuditDraft, IncidentRepo, IncidentTimelineEntry, JournalDraft,
    JournalKind, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{ActionRequest, Actor, Incident, Risk, UserId};
use serde_json::json;

use super::snapshot::Snapshot;
use super::{
    CrashPlan, CrashPoint, CrashingStore, Exempt, check_invariants, check_invariants_except,
};
use crate::build::{T0, at, incident as pending_incident, signal};
use crate::repos::InMemoryStore;

fn host() -> domovoi_core::HostId {
    domovoi_core::HostId::new()
}

fn new_incident_tx(incident: &Incident) -> Transaction {
    Transaction::new().write(StateWrite::Incident {
        incident: incident.clone(),
        expected: domovoi_core::ports::Expected::New,
    })
}

async fn stored_incident(store: &InMemoryStore, id: domovoi_core::IncidentId) -> bool {
    IncidentRepo::get(store, id).await.unwrap().is_some()
}

#[tokio::test]
async fn before_commit_crash_applies_nothing_and_plays_dead() {
    let inner = Arc::new(InMemoryStore::new());
    let crashing = CrashingStore::new(inner.clone());
    let incident = pending_incident(host(), "disk.smart", None).unwrap();
    let id = incident.id();

    crashing.arm(CrashPlan {
        at_commit: 1,
        point: CrashPoint::BeforeCommit,
    });
    let err = crashing
        .commit(new_incident_tx(&incident))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        domovoi_core::ports::CommitError::Backend("simulated crash before commit".to_owned())
    );
    assert!(!stored_incident(&inner, id).await);
    assert!(crashing.crashed());

    let second = pending_incident(host(), "disk.full", None).unwrap();
    let err = crashing.commit(new_incident_tx(&second)).await.unwrap_err();
    assert_eq!(
        err,
        domovoi_core::ports::CommitError::Backend("process is down".to_owned())
    );
}

#[tokio::test]
async fn after_commit_crash_applies_everything_and_hides_the_receipt() {
    let inner = Arc::new(InMemoryStore::new());
    let crashing = CrashingStore::new(inner.clone());
    let incident = pending_incident(host(), "disk.smart", None).unwrap();

    crashing.arm(CrashPlan {
        at_commit: 1,
        point: CrashPoint::AfterCommit,
    });
    let err = crashing
        .commit(new_incident_tx(&incident))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        domovoi_core::ports::CommitError::Backend("simulated crash after commit".to_owned())
    );
    assert!(stored_incident(&inner, incident.id()).await);
    assert!(crashing.crashed());
}

#[tokio::test]
async fn restart_makes_the_store_usable_again() {
    let inner = Arc::new(InMemoryStore::new());
    let crashing = CrashingStore::new(inner.clone());
    let first = pending_incident(host(), "disk.smart", None).unwrap();
    crashing.arm(CrashPlan {
        at_commit: 1,
        point: CrashPoint::AfterCommit,
    });
    let _ = crashing.commit(new_incident_tx(&first)).await;
    assert!(crashing.crashed());

    crashing.restart();
    assert!(!crashing.crashed());
    let second = pending_incident(host(), "disk.full", None).unwrap();
    crashing.commit(new_incident_tx(&second)).await.unwrap();
    assert_eq!(crashing.summaries().len(), 2);
    assert_eq!(crashing.summaries()[0].writes, vec!["incident:new"]);
    assert_eq!(crashing.commits(), 1);
}

#[tokio::test]
async fn only_the_planned_commit_crashes() {
    let crashing = CrashingStore::new(Arc::new(InMemoryStore::new()));
    crashing.arm(CrashPlan {
        at_commit: 3,
        point: CrashPoint::BeforeCommit,
    });
    for check in ["a", "b"] {
        let incident = pending_incident(host(), check, None).unwrap();
        crashing.commit(new_incident_tx(&incident)).await.unwrap();
    }
    assert!(!crashing.crashed());

    let third = pending_incident(host(), "c", None).unwrap();
    let err = crashing.commit(new_incident_tx(&third)).await.unwrap_err();
    assert!(matches!(
        err,
        domovoi_core::ports::CommitError::Backend(ref why)
            if why == "simulated crash before commit"
    ));
    assert!(crashing.crashed());
    assert_eq!(crashing.commits(), 3);
}

#[tokio::test]
async fn empty_transactions_are_not_counted() {
    let crashing = CrashingStore::new(Arc::new(InMemoryStore::new()));
    crashing.arm(CrashPlan {
        at_commit: 1,
        point: CrashPoint::BeforeCommit,
    });
    crashing.commit(Transaction::new()).await.unwrap();
    assert_eq!(crashing.commits(), 0);
    assert!(crashing.summaries().is_empty());
    assert!(!crashing.crashed());
}

#[tokio::test]
async fn summaries_describe_each_commit_without_ids() {
    let crashing = CrashingStore::new(Arc::new(InMemoryStore::new()));
    crashing.record_only();
    let host = host();
    let signal = signal(host, "cpu.load", None).unwrap();
    let incident = pending_incident(host, "cpu.load", None).unwrap();
    let incident_id = incident.id();
    let entry = IncidentTimelineEntry {
        incident: incident.id(),
        at: T0,
        kind: "status.changed".to_owned(),
        summary: "opened".to_owned(),
        detail: json!({"from": "pending"}),
    };
    let action = crate::build::action(incident.id(), host, Risk::Write).unwrap();
    let action_id = action.id();
    let tx = Transaction::new()
        .write(StateWrite::Signal {
            signal,
            incident: Some(incident.id()),
        })
        .write(StateWrite::Incident {
            incident,
            expected: domovoi_core::ports::Expected::New,
        })
        .write(StateWrite::IncidentEvent(entry))
        .write(StateWrite::Action {
            action,
            expected: ActionGuard::New,
        })
        .journal(JournalDraft {
            aggregate: AggregateRef::action(domovoi_core::ActionId::new()),
            kind: JournalKind::ActionTransition,
            payload_v: 1,
            payload: json!({"to": "proposed"}),
            actor: Actor::System("test".to_owned()),
            occurred_at: T0,
        })
        .audit(AuditDraft::new(
            Actor::System("test".to_owned()),
            T0,
            "action.proposed".to_owned(),
            "action:x".to_owned(),
            json!({}),
        ));
    crashing.commit(tx).await.unwrap();

    let summary = &crashing.summaries()[0];
    assert_eq!(
        summary.writes,
        vec!["signal", "incident:new", "incident_event", "action:new"]
    );
    assert_eq!(
        summary.journal,
        vec!["incident.timeline", "action.transition"]
    );
    assert_eq!(summary.audit, vec!["action.proposed"]);
    let rendered = format!("{summary:?}");
    assert!(!rendered.contains(&incident_id.to_string()), "{rendered}");
    assert!(!rendered.contains(&action_id.to_string()), "{rendered}");
}

#[tokio::test]
async fn snapshot_is_independent_of_ids_and_order() {
    let first = Arc::new(InMemoryStore::new());
    let one = pending_incident(host(), "disk.smart", None).unwrap();
    IncidentRepo::insert(&*first, &one).await.unwrap();
    domovoi_core::ports::ActionRepo::insert(
        &*first,
        &crate::build::action(one.id(), one.host(), Risk::Write).unwrap(),
    )
    .await
    .unwrap();

    let second = Arc::new(InMemoryStore::new());
    let other_host = host();
    let two = pending_incident(other_host, "disk.smart", None).unwrap();
    domovoi_core::ports::ActionRepo::insert(
        &*second,
        &crate::build::action(two.id(), other_host, Risk::Write).unwrap(),
    )
    .await
    .unwrap();
    IncidentRepo::insert(&*second, &two).await.unwrap();

    assert_eq!(
        Snapshot::capture(&*first).await.unwrap(),
        Snapshot::capture(&*second).await.unwrap()
    );
}

#[tokio::test]
async fn snapshots_distinguish_states() {
    let store = Arc::new(InMemoryStore::new());
    let mut incident = pending_incident(host(), "disk.smart", None).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    let before = Snapshot::capture(&*store).await.unwrap();

    domovoi_core::apply(
        &mut incident,
        domovoi_core::IncidentEvent::Firing {
            severity: domovoi_core::Severity::Warning,
            class: domovoi_core::SignalClass::Immediate,
            observed_at: T0,
            value: None,
        },
        &domovoi_core::IncidentPolicy::default(),
        T0,
    )
    .unwrap();
    IncidentRepo::update(&*store, &incident).await.unwrap();

    let after = Snapshot::capture(&*store).await.unwrap();
    assert_ne!(before, after);
    assert_eq!(
        after.incidents,
        vec![("disk.smart".to_owned(), "open".to_owned())]
    );
}

/// One incident with a journaled timeline entry, and one action proposed and approved with
/// its journal and audit rows: the shape the writers produce.
async fn consistent_store() -> (Arc<InMemoryStore>, Incident, ActionRequest) {
    let store = Arc::new(InMemoryStore::new());
    let host = host();
    let signal = signal(host, "disk.smart", None).unwrap();
    let incident = pending_incident(host, "disk.smart", None).unwrap();
    let entry = IncidentTimelineEntry {
        incident: incident.id(),
        at: T0,
        kind: "status.changed".to_owned(),
        summary: "opened".to_owned(),
        detail: json!({"from": "pending", "to": "open"}),
    };
    let mut action = crate::build::action(incident.id(), host, Risk::Write).unwrap();
    let proposed = Transaction::new()
        .write(StateWrite::Signal {
            signal,
            incident: Some(incident.id()),
        })
        .write(StateWrite::Incident {
            incident: incident.clone(),
            expected: domovoi_core::ports::Expected::New,
        })
        .write(StateWrite::IncidentEvent(entry))
        .write(StateWrite::Action {
            action: action.clone(),
            expected: ActionGuard::New,
        })
        .journal(transition_journal(&action, "proposed"))
        .audit(proposal_audit(&action));
    store.commit(proposed).await.unwrap();

    action
        .approve(crate::approvals::human(UserId::new()), at(1))
        .unwrap();
    let approved = Transaction::new()
        .write(StateWrite::Action {
            action: action.clone(),
            expected: ActionGuard::Status("proposed".to_owned()),
        })
        .journal(transition_journal(&action, "approved"))
        .audit(AuditDraft::new(
            Actor::System("test".to_owned()),
            at(1),
            "action.approved".to_owned(),
            format!("action:{}", action.id()),
            json!({}),
        ));
    store.commit(approved).await.unwrap();
    (store, incident, action)
}

fn transition_journal(action: &ActionRequest, to: &str) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::action(action.id()),
        kind: JournalKind::ActionTransition,
        payload_v: 1,
        payload: json!({"to": to}),
        actor: Actor::System("test".to_owned()),
        occurred_at: T0,
    }
}

fn proposal_audit(action: &ActionRequest) -> AuditDraft {
    AuditDraft::new(
        Actor::System("test".to_owned()),
        T0,
        "action.proposed".to_owned(),
        format!("action:{}", action.id()),
        json!({}),
    )
}

#[tokio::test]
async fn invariants_accept_consistent_state() {
    let (store, _, _) = consistent_store().await;
    check_invariants(&*store).await.unwrap();
}

#[tokio::test]
async fn invariants_report_an_action_without_its_audit_row() {
    let store = Arc::new(InMemoryStore::new());
    let host = host();
    let incident = pending_incident(host, "disk.smart", None).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    let action = crate::build::action(incident.id(), host, Risk::Write).unwrap();
    store
        .commit(
            Transaction::new()
                .write(StateWrite::Action {
                    action: action.clone(),
                    expected: ActionGuard::New,
                })
                .journal(transition_journal(&action, "proposed")),
        )
        .await
        .unwrap();

    let violations = check_invariants(&*store).await.unwrap_err();
    assert!(
        violations
            .iter()
            .any(|violation| violation.starts_with("I2:")),
        "{violations:?}"
    );
}

#[tokio::test]
async fn invariants_report_a_timeline_entry_without_its_journal_row() {
    let store = Arc::new(InMemoryStore::new());
    let host = host();
    let incident = pending_incident(host, "disk.smart", None).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    IncidentRepo::append_event(
        &*store,
        &IncidentTimelineEntry {
            incident: incident.id(),
            at: T0,
            kind: "legacy".to_owned(),
            summary: "not journaled".to_owned(),
            detail: json!(null),
        },
    )
    .await
    .unwrap();

    let violations = check_invariants(&*store).await.unwrap_err();
    assert!(
        violations
            .iter()
            .any(|violation| violation.starts_with("I3:")),
        "{violations:?}"
    );
    check_invariants_except(
        &*store,
        &Exempt {
            actions: Default::default(),
            legacy_timeline: [(incident.id(), 1)].into_iter().collect(),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn invariants_report_an_action_whose_journal_disagrees_with_its_status() {
    let store = Arc::new(InMemoryStore::new());
    let host = host();
    let incident = pending_incident(host, "disk.smart", None).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    let action = crate::build::action(incident.id(), host, Risk::Write).unwrap();
    store
        .commit(
            Transaction::new()
                .write(StateWrite::Action {
                    action: action.clone(),
                    expected: ActionGuard::New,
                })
                .journal(transition_journal(&action, "proposed"))
                .audit(proposal_audit(&action)),
        )
        .await
        .unwrap();

    let mut approved = action.clone();
    approved
        .approve(crate::approvals::human(UserId::new()), at(1))
        .unwrap();
    domovoi_core::ports::ActionRepo::update_from(&*store, &approved, "proposed")
        .await
        .unwrap();

    let violations = check_invariants(&*store).await.unwrap_err();
    assert!(
        violations
            .iter()
            .any(|violation| violation.starts_with("I1:")),
        "{violations:?}"
    );
}

#[tokio::test]
async fn missing_actions_are_reported_as_store_errors_not_consistent() {
    let store = Arc::new(InMemoryStore::new());
    let incident = pending_incident(host(), "disk.smart", None).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    store.fail_next(StoreError::Backend("disk".to_owned()));
    assert!(check_invariants(&*store).await.is_err());
}
