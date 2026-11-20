//! Management paths: confirmation against verified coverage, and network changes.

use domovoi_core::knowledge::{
    FactSource, KnowledgeStatus, PathState, Predicate, SubjectRef, UnverifiableReason,
};
use domovoi_core::ports::{JournalRead, KnowledgeRead};
use domovoi_core::{HostId, UserId};
use domovoi_engine::knowledge::{KnowledgeError, NetworkChangeCause};
use serde_json::json;

use super::knowledge::{all_audit, audit, draft, journal_len, owner_fact, rig, system};

fn host_with_id() -> (SubjectRef, HostId) {
    let id = HostId::new();
    (SubjectRef::Host(id), id)
}

const CIDR: &str = "192.168.1.0/24";

#[tokio::test]
async fn confirming_a_path_needs_verified_coverage() {
    let r = rig();
    r.coverage.cover(&[CIDR]);
    let (s, h) = host_with_id();
    let p = owner_fact(&r, &s, Predicate::ManagementPath, json!({"cidr": CIDR})).await;
    let view = r
        .service
        .confirm_management_path(p.fact.value.id, p.fact.version, UserId::new())
        .await
        .unwrap();
    assert_eq!(view.state, PathState::Confirmed);
    assert_eq!(view.fact.value.status, KnowledgeStatus::Current);
    let c = view.confirmation.unwrap();
    assert_eq!(c.evidence.as_str(), "fake-evidence");
    assert_eq!(c.host, h);
    assert_eq!(
        audit(&r, "knowledge.management_path.confirmed").await.len(),
        1
    );
    let entries = r.store.entries_after(0, 100).await.unwrap();
    assert!(entries.iter().any(|e| e.payload["change"] == "confirmed"));
    assert_eq!(r.coverage.calls(), vec![(h, CIDR.to_owned())]);
}

#[tokio::test]
async fn owner_cannot_confirm_uncovered_management_path() {
    let r = rig();
    r.coverage.cover(&["10.9.9.0/24"]);
    let (s, _) = host_with_id();
    let p = owner_fact(&r, &s, Predicate::ManagementPath, json!({"cidr": CIDR})).await;
    let (journal, audits) = (journal_len(&r).await, all_audit(&r).await.len());
    let err = r
        .service
        .confirm_management_path(p.fact.value.id, p.fact.version, UserId::new())
        .await
        .unwrap_err();
    assert_eq!(err, KnowledgeError::PathUncovered);
    assert!(
        r.store
            .confirmation(p.fact.value.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        (journal_len(&r).await, all_audit(&r).await.len()),
        (journal, audits)
    );
}

#[tokio::test]
async fn unavailable_coverage_refuses_every_confirmation() {
    let r = rig();
    assert!(!r.service.coverage_available());
    let (s, _) = host_with_id();
    let p = owner_fact(&r, &s, Predicate::ManagementPath, json!({"cidr": CIDR})).await;
    let (journal, audits) = (journal_len(&r).await, all_audit(&r).await.len());
    let err = r
        .service
        .confirm_management_path(p.fact.value.id, p.fact.version, UserId::new())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        KnowledgeError::CoverageUnverifiable(UnverifiableReason::NotAvailable)
    );
    assert!(
        r.store
            .confirmation(p.fact.value.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        (journal_len(&r).await, all_audit(&r).await.len()),
        (journal, audits)
    );
}

#[tokio::test]
async fn unverifiable_coverage_reasons_are_passed_through() {
    let r = rig();
    r.coverage
        .set_unverifiable(UnverifiableReason::AttestationStale);
    let (s, _) = host_with_id();
    let p = owner_fact(&r, &s, Predicate::ManagementPath, json!({"cidr": CIDR})).await;
    let err = r
        .service
        .confirm_management_path(p.fact.value.id, p.fact.version, UserId::new())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        KnowledgeError::CoverageUnverifiable(UnverifiableReason::AttestationStale)
    );
}

#[tokio::test]
async fn confirming_a_discovered_path_is_refused_until_the_owner_accepts_it() {
    let r = rig();
    r.coverage.cover(&[CIDR]);
    let (s, _) = host_with_id();
    let found = r
        .service
        .record(
            draft(
                &s,
                Predicate::ManagementPath,
                json!({"cidr": CIDR}),
                FactSource::Discovered,
            ),
            &system(),
        )
        .await
        .unwrap();
    let err = r
        .service
        .confirm_management_path(found.fact.value.id, found.fact.version, UserId::new())
        .await
        .unwrap_err();
    assert_eq!(err, KnowledgeError::NotOwnerFact);
    let accepted = r
        .service
        .accept(found.fact.value.id, found.fact.version, UserId::new())
        .await
        .unwrap();
    let view = r
        .service
        .confirm_management_path(accepted.fact.value.id, accepted.fact.version, UserId::new())
        .await
        .unwrap();
    assert_eq!(view.state, PathState::Confirmed);
}

#[tokio::test]
async fn confirming_something_that_is_not_a_path_is_invalid() {
    let r = rig();
    let (s, _) = host_with_id();
    let role = owner_fact(&r, &s, Predicate::Role, json!("nas")).await;
    let err = r
        .service
        .confirm_management_path(role.fact.value.id, role.fact.version, UserId::new())
        .await
        .unwrap_err();
    assert!(matches!(err, KnowledgeError::InvalidInput(_)));
}

#[tokio::test]
async fn network_change_marks_management_paths_stale() {
    let r = rig();
    r.coverage.cover(&[CIDR]);
    let (s, h) = host_with_id();
    let p = owner_fact(&r, &s, Predicate::ManagementPath, json!({"cidr": CIDR})).await;
    r.service
        .confirm_management_path(p.fact.value.id, p.fact.version, UserId::new())
        .await
        .unwrap();
    let changed = r
        .service
        .note_network_change(h, NetworkChangeCause::PeerAddressChanged)
        .await
        .unwrap();
    assert_eq!(changed, vec![p.fact.value.id]);
    let stored = r.store.fact(p.fact.value.id).await.unwrap().unwrap();
    assert_eq!(stored.value.status, KnowledgeStatus::Stale);
    let views = r.service.management_paths(h).await.unwrap();
    assert_eq!(views[0].state, PathState::Stale);
    assert!(views[0].confirmation.is_some());
    let events = audit(&r, "knowledge.management_path.stale").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail["cause"], "peer_address_changed");
    assert_eq!(events[0].actor, "system:knowledge");
    assert!(
        r.service
            .note_network_change(h, NetworkChangeCause::Other)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(audit(&r, "knowledge.management_path.stale").await.len(), 1);
}

#[tokio::test]
async fn management_paths_are_listed_by_range() {
    let r = rig();
    let (s, h) = host_with_id();
    owner_fact(
        &r,
        &s,
        Predicate::ManagementPath,
        json!({"cidr": "192.168.2.0/24"}),
    )
    .await;
    owner_fact(
        &r,
        &s,
        Predicate::ManagementPath,
        json!({"cidr": "10.0.0.0/8"}),
    )
    .await;
    let views = r.service.management_paths(h).await.unwrap();
    let cidrs: Vec<_> = views
        .iter()
        .map(|v| {
            v.fact.value.value.clone().unwrap()["cidr"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(cidrs, ["10.0.0.0/8", "192.168.2.0/24"]);
    assert!(views.iter().all(|v| v.state == PathState::Unconfirmed));
}
