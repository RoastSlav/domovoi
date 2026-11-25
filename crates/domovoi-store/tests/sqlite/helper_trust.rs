//! Helper key pins, attestation outcomes and high-water marks, in SQLite and in the in-memory
//! twin: the acceptance transaction, the conflict guard, pin and unpin, host removal, and what
//! survives a restart.

use domovoi_core::attest::{LatestOutcome, Nonce32, SafetyIdentity, UnverifiedReason};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{
    Accepted, AuditDraft, AuditFilter, AuditLog, CommitError, HelperTrustRepo, HelperTrustWrite,
    HighWater, OutcomeRecord, Page, PinWrite, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{Actor, HostId};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::{T0, at};
use domovoi_testkit::helper_attest::complete_report;
use serde_json::json;

use crate::common::Fresh;

const KEY: [u8; 32] = [5; 32];

fn fingerprint() -> KeyFingerprint {
    KeyFingerprint::of_public_key(&KEY)
}

fn pin(host: HostId) -> Transaction {
    Transaction::new().write(StateWrite::HelperTrust(HelperTrustWrite::Pin(PinWrite {
        host,
        public_key: KEY,
        fingerprint: fingerprint(),
        pinned_at: at(1),
        pinned_by: "user:test".to_owned(),
    })))
}

fn accepted(
    host: HostId,
    generation: u64,
    variant: u64,
    seen: Option<HighWater>,
) -> Accepted {
    let report = complete_report(Nonce32::from_bytes([1; 32]), generation, variant).unwrap();
    let bytes = domovoi_core::attest::render_complete(&report);
    Accepted {
        host,
        identity: SafetyIdentity {
            fingerprint: fingerprint(),
            boot_id: report.boot_id.clone(),
            generation,
            config_hash: report.config_hash,
            route_state_hash: report.route_state_hash,
        },
        report: bytes,
        nonce_issued_at: at(100),
        accepted_at: at(101),
        seen_high_water: seen,
    }
}

fn accept(accepted: Accepted) -> Transaction {
    Transaction::new().write(StateWrite::HelperTrust(HelperTrustWrite::Accept(accepted)))
}

async fn mark(store: &impl HelperTrustRepo, host: HostId) -> Option<HighWater> {
    store.high_water(host, &fingerprint()).await.unwrap()
}

async fn accept_stores_the_report_and_the_mark(store: &(impl UnitOfWork + HelperTrustRepo)) {
    let host = HostId::new();
    store.commit(pin(host)).await.unwrap();
    store
        .commit(accept(accepted(host, 3, 0, None)))
        .await
        .unwrap();
    let stored = store.stored(host).await.unwrap();
    let verified = stored.view.verified.expect("a verified state");
    assert_eq!(verified.identity.generation, 3);
    assert_eq!(verified.identity.fingerprint, fingerprint());
    assert_eq!(verified.nonce_issued_at, at(100));
    assert_eq!(verified.max_active, 0);
    assert_eq!(stored.view.latest, Some(LatestOutcome::Verified));
    assert_eq!(mark(store, host).await.map(|row| row.generation), Some(3));
}

async fn a_moved_mark_is_a_conflict_and_nothing_is_written(
    store: &(impl UnitOfWork + HelperTrustRepo),
) {
    let host = HostId::new();
    store.commit(pin(host)).await.unwrap();
    // Two verifications of one host read the same (empty) mark; only one may advance it.
    store
        .commit(accept(accepted(host, 1, 0, None)))
        .await
        .unwrap();
    let second = store.commit(accept(accepted(host, 2, 0, None))).await;
    assert!(
        matches!(second, Err(CommitError::Conflict { .. })),
        "{second:?}"
    );
    assert_eq!(mark(store, host).await.map(|row| row.generation), Some(1));
    // Steps 8 to 10 are re-checked inside the commit even with a current view.
    let seen = mark(store, host).await;
    let lower = store.commit(accept(accepted(host, 0, 0, seen.clone()))).await;
    assert!(matches!(lower, Err(CommitError::Conflict { .. })));
    let repeat = store.commit(accept(accepted(host, 1, 1, seen.clone()))).await;
    assert!(matches!(repeat, Err(CommitError::Conflict { .. })));
    store
        .commit(accept(accepted(host, 1, 0, seen)))
        .await
        .unwrap();
}

async fn an_outcome_revokes_and_pin_and_unpin_move_the_marks(
    store: &(impl UnitOfWork + HelperTrustRepo),
) {
    let host = HostId::new();
    store.commit(pin(host)).await.unwrap();
    store
        .commit(accept(accepted(host, 4, 0, None)))
        .await
        .unwrap();
    let record = OutcomeRecord {
        host,
        outcome: LatestOutcome::SignedIncomplete,
        reason: UnverifiedReason::ReportIncomplete,
        detail: "helper_incomplete:state_corrupt".to_owned(),
        at: at(200),
    };
    store
        .commit(Transaction::new().write(StateWrite::HelperTrust(
            HelperTrustWrite::RecordOutcome(record),
        )))
        .await
        .unwrap();
    let stored = store.stored(host).await.unwrap();
    assert_eq!(stored.view.latest, Some(LatestOutcome::SignedIncomplete));
    assert!(stored.view.verified.is_none(), "the outcome revokes");
    let last = stored.last.unwrap();
    assert_eq!(last.reason, Some(UnverifiedReason::ReportIncomplete));
    assert_eq!(last.detail.as_deref(), Some("helper_incomplete:state_corrupt"));
    assert_eq!(mark(store, host).await.map(|row| row.generation), Some(4));

    // An unpin clears the pin and the outcome and keeps the mark.
    store
        .commit(Transaction::new().write(StateWrite::HelperTrust(HelperTrustWrite::Unpin {
            host,
        })))
        .await
        .unwrap();
    let stored = store.stored(host).await.unwrap();
    assert!(stored.view.pin.is_none() && stored.view.latest.is_none());
    assert_eq!(mark(store, host).await.map(|row| row.generation), Some(4));

    // An owner re-pin resets the mark of that key and starts with no outcome.
    store.commit(pin(host)).await.unwrap();
    assert!(mark(store, host).await.is_none());
    assert!(store.stored(host).await.unwrap().view.latest.is_none());
}

async fn removing_a_host_deletes_its_rows(store: &(impl UnitOfWork + HelperTrustRepo)) {
    let host = HostId::new();
    let other = HostId::new();
    for id in [host, other] {
        store.commit(pin(id)).await.unwrap();
        store
            .commit(accept(accepted(id, 2, 0, None)))
            .await
            .unwrap();
    }
    store
        .commit(Transaction::new().write(StateWrite::HelperTrust(
            HelperTrustWrite::RemoveHost { host },
        )))
        .await
        .unwrap();
    let gone = store.stored(host).await.unwrap();
    assert!(gone.view.pin.is_none() && gone.view.verified.is_none());
    assert!(mark(store, host).await.is_none());
    assert!(store.stored(other).await.unwrap().view.pin.is_some());
    assert!(mark(store, other).await.is_some());
}

async fn a_verified_outcome_cannot_be_recorded_directly(
    store: &(impl UnitOfWork + HelperTrustRepo),
) {
    let record = OutcomeRecord {
        host: HostId::new(),
        outcome: LatestOutcome::Verified,
        reason: UnverifiedReason::AttestationMissing,
        detail: String::new(),
        at: at(1),
    };
    let result = store
        .commit(Transaction::new().write(StateWrite::HelperTrust(
            HelperTrustWrite::RecordOutcome(record),
        )))
        .await;
    assert!(matches!(result, Err(CommitError::Constraint(_))));
}

#[tokio::test]
async fn high_water_written_in_the_acceptance_transaction() {
    let fresh = Fresh::new().await;
    accept_stores_the_report_and_the_mark(&fresh.store).await;
    accept_stores_the_report_and_the_mark(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn concurrent_acceptance_cannot_advance_the_mark_twice() {
    let fresh = Fresh::new().await;
    a_moved_mark_is_a_conflict_and_nothing_is_written(&fresh.store).await;
    a_moved_mark_is_a_conflict_and_nothing_is_written(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn pin_unpin_and_outcomes_move_the_stored_trust() {
    let fresh = Fresh::new().await;
    an_outcome_revokes_and_pin_and_unpin_move_the_marks(&fresh.store).await;
    an_outcome_revokes_and_pin_and_unpin_move_the_marks(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn host_removal_deletes_pin_state_and_high_water() {
    let fresh = Fresh::new().await;
    removing_a_host_deletes_its_rows(&fresh.store).await;
    removing_a_host_deletes_its_rows(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn only_an_accepted_report_stores_a_verified_outcome() {
    let fresh = Fresh::new().await;
    a_verified_outcome_cannot_be_recorded_directly(&fresh.store).await;
    a_verified_outcome_cannot_be_recorded_directly(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn high_water_persisted_across_hub_restart() {
    let fresh = Fresh::new().await;
    let host = HostId::new();
    fresh.store.commit(pin(host)).await.unwrap();
    fresh
        .store
        .commit(accept(accepted(host, 7, 0, None)))
        .await
        .unwrap();
    let path = fresh.path();
    fresh.store.close().await;
    let reopened = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .unwrap();
    assert_eq!(mark(&reopened, host).await.map(|row| row.generation), Some(7));
    let stored = reopened.stored(host).await.unwrap();
    assert_eq!(stored.view.pin.unwrap().public_key, KEY);
    assert_eq!(stored.view.verified.unwrap().identity.generation, 7);
    // The mark still refuses a lower generation after the restart.
    let lower = reopened
        .commit(accept(accepted(host, 6, 0, mark(&reopened, host).await)))
        .await;
    assert!(matches!(lower, Err(CommitError::Conflict { .. })));
}

#[tokio::test]
async fn a_failed_commit_leaves_its_audit_row_unwritten_too() {
    let fresh = Fresh::new().await;
    let host = HostId::new();
    let audit = AuditDraft::new(
        Actor::System("helper-trust".to_owned()),
        at(5),
        "helper.attestation_restored".to_owned(),
        format!("host:{host}"),
        json!({"generation": 9}),
    );
    // The mark moved (a row exists that this write did not see), so the whole commit fails.
    fresh.store.commit(pin(host)).await.unwrap();
    fresh
        .store
        .commit(accept(accepted(host, 1, 0, None)))
        .await
        .unwrap();
    let tx = accept(accepted(host, 2, 0, None)).audit(audit);
    assert!(fresh.store.commit(tx).await.is_err());
    let rows = fresh
        .store
        .list(
            &AuditFilter {
                kind: Some("helper.attestation_restored".to_owned()),
                ..AuditFilter::default()
            },
            &Page::first(10),
        )
        .await
        .unwrap();
    assert!(rows.items.is_empty());
}
