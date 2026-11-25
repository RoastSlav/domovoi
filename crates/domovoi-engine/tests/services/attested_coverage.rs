//! Coverage decided from the helper's verified attestation: what is covered, what is not, why
//! it cannot be known, and how an owner's confirmation ages against a changing safety set.

use std::sync::Arc;

use domovoi_core::attest::{LatestOutcome, SafetyIdentity, UnverifiedReason};
use domovoi_core::knowledge::{
    CoverageEvidence, CoverageVerdict, FactSource, PathState, Predicate, SubjectRef,
    UnverifiableReason,
};
use domovoi_core::ports::{Clock, ManagementCoverage};
use domovoi_core::{HostId, UserId};
use domovoi_engine::knowledge::{
    AttestedCoverage, KnowledgeDeps, KnowledgeError, KnowledgeService, UnavailableCoverage,
};
use domovoi_testkit::build::T0;
use domovoi_testkit::helper_attest::{FakeHelperTrust, TestHelper, verified_view};
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use super::knowledge::draft;

const COVERED: &str = "192.0.2.0/25";
const OUTSIDE: &str = "198.51.100.0/24";

struct Rig {
    trust: Arc<FakeHelperTrust>,
    clock: Arc<FakeClock>,
    helper: TestHelper,
    host: HostId,
    coverage: AttestedCoverage,
}

fn rig() -> Rig {
    let trust = Arc::new(FakeHelperTrust::new());
    let clock = Arc::new(FakeClock::new(T0));
    Rig {
        coverage: AttestedCoverage::new(trust.clone(), clock.clone()),
        trust,
        clock,
        helper: TestHelper::new(1),
        host: HostId::new(),
    }
}

impl Rig {
    fn verified(&self, generation: u64, variant: u64) {
        self.trust.set(
            self.host,
            verified_view(&self.helper, generation, variant, self.clock.now()).unwrap(),
        );
    }

    fn service(&self) -> (KnowledgeService, Arc<InMemoryStore>) {
        let store = Arc::new(InMemoryStore::new());
        let service = KnowledgeService::new(KnowledgeDeps {
            uow: store.clone(),
            read: store.clone(),
            coverage: Arc::new(AttestedCoverage::new(self.trust.clone(), self.clock.clone())),
            clock: self.clock.clone(),
        });
        (service, store)
    }

    async fn path(&self, service: &KnowledgeService, cidr: &str) -> domovoi_engine::knowledge::Recorded {
        service
            .assert_fact(
                draft(
                    &SubjectRef::Host(self.host),
                    Predicate::ManagementPath,
                    json!({"cidr": cidr}),
                    FactSource::User,
                ),
                UserId::new(),
            )
            .await
            .unwrap()
    }

    async fn confirm(
        &self,
        service: &KnowledgeService,
        path: &domovoi_engine::knowledge::Recorded,
    ) -> Result<domovoi_engine::knowledge::ManagementPathView, KnowledgeError> {
        service
            .confirm_management_path(path.fact.value.id, path.fact.version, UserId::new())
            .await
    }
}

fn evidence_of(verdict: CoverageVerdict) -> CoverageEvidence {
    match verdict {
        CoverageVerdict::Covered { evidence } => evidence,
        other => panic!("expected Covered, got {other:?}"),
    }
}

#[tokio::test]
async fn attested_coverage_returns_covered_uncovered_and_unverifiable() {
    let rig = rig();
    assert!(rig.coverage.is_available());
    // A host never seen has no pin.
    assert_eq!(
        rig.coverage.verdict(rig.host, COVERED).await,
        CoverageVerdict::Unverifiable {
            reason: UnverifiableReason::KeyNotPinned
        }
    );
    rig.verified(1, 0);
    let evidence = evidence_of(rig.coverage.verdict(rig.host, COVERED).await);
    let identity = SafetyIdentity::parse_canonical(evidence.as_str()).unwrap();
    assert_eq!(identity.generation, 1);
    assert_eq!(identity.fingerprint, rig.helper.fingerprint());
    // A covered range given with host bits set is normalised; the hub's own address is covered.
    evidence_of(rig.coverage.verdict(rig.host, "192.0.2.77/24").await);
    evidence_of(rig.coverage.verdict(rig.host, "192.0.2.10/32").await);
    assert_eq!(
        rig.coverage.verdict(rig.host, OUTSIDE).await,
        CoverageVerdict::Uncovered
    );
    assert_eq!(
        rig.coverage.verdict(rig.host, "not a range").await,
        CoverageVerdict::Uncovered
    );
    rig.clock.advance(SignedDuration::from_secs(300));
    assert_eq!(
        rig.coverage.verdict(rig.host, COVERED).await,
        CoverageVerdict::Unverifiable {
            reason: UnverifiableReason::AttestationStale
        }
    );
    let mut mismatch = verified_view(&rig.helper, 1, 0, rig.clock.now()).unwrap();
    mismatch.latest = Some(LatestOutcome::Mismatch);
    rig.trust.set(rig.host, mismatch);
    assert_eq!(
        rig.coverage.verdict(rig.host, COVERED).await,
        CoverageVerdict::Unverifiable {
            reason: UnverifiableReason::KeyMismatch
        }
    );
}

#[tokio::test]
async fn agent_cannot_fabricate_effective_safety_set() {
    // An agent can relay a key and say anything about its interfaces and routes. None of it
    // is a verified state, so nothing is ever covered.
    let rig = rig();
    let mut view = domovoi_core::attest::TrustView {
        received_key: Some(rig.helper.public_key()),
        ..Default::default()
    };
    rig.trust.set(rig.host, view.clone());
    for cidr in ["0.0.0.0/0", COVERED, OUTSIDE, "::/0"] {
        assert!(matches!(
            rig.coverage.verdict(rig.host, cidr).await,
            CoverageVerdict::Unverifiable { .. }
        ));
    }
    // Even with the key pinned, a missing verified state covers nothing.
    view.pin = verified_view(&rig.helper, 1, 0, T0).unwrap().pin;
    rig.trust.set(rig.host, view);
    assert_eq!(
        rig.coverage.verdict(rig.host, COVERED).await,
        CoverageVerdict::Unverifiable {
            reason: UnverifiableReason::AttestationMissing
        }
    );
}

#[tokio::test]
async fn confirmation_current_follows_the_reboot_exception_only() {
    let rig = rig();
    rig.verified(1, 0);
    let evidence = evidence_of(rig.coverage.verdict(rig.host, COVERED).await);
    let now = rig.clock.now();
    assert_eq!(
        rig.coverage.confirmation_current(rig.host, &evidence, now).await,
        Ok(())
    );
    // The same identity but another boot: a reboot alone does not invalidate it.
    let mut identity = SafetyIdentity::parse_canonical(evidence.as_str()).unwrap();
    identity.boot_id =
        domovoi_core::attest::BootId::parse("0a0b0c0d-1111-4222-8333-444455556666").unwrap();
    let rebooted = CoverageEvidence::new(identity.canonical()).unwrap();
    assert_eq!(
        rig.coverage.confirmation_current(rig.host, &rebooted, now).await,
        Ok(())
    );
    // A changed generation or configuration does.
    let stale = Err(UnverifiableReason::ConfirmationStale);
    rig.verified(2, 1);
    assert_eq!(
        rig.coverage.confirmation_current(rig.host, &evidence, now).await,
        stale
    );
    // An unreadable evidence text and an old confirmation do too.
    rig.verified(1, 0);
    let junk = CoverageEvidence::new("fake-evidence").unwrap();
    assert_eq!(
        rig.coverage.confirmation_current(rig.host, &junk, now).await,
        stale
    );
    let thirty_days = SignedDuration::from_secs(30 * 86_400);
    let old = now.checked_sub(thirty_days).unwrap();
    assert_eq!(
        rig.coverage.confirmation_current(rig.host, &evidence, old).await,
        stale
    );
    // Without a current verified attestation the reason is the gate's.
    rig.clock.advance(SignedDuration::from_secs(300));
    assert_eq!(
        rig.coverage.confirmation_current(rig.host, &evidence, now).await,
        Err(UnverifiableReason::AttestationStale)
    );
}

#[tokio::test]
async fn unavailable_coverage_still_refuses_every_confirmation() {
    let coverage = UnavailableCoverage;
    let evidence = CoverageEvidence::new("anything").unwrap();
    assert!(!coverage.is_available());
    assert_eq!(
        coverage.verdict(HostId::new(), COVERED).await,
        CoverageVerdict::Unverifiable {
            reason: UnverifiableReason::NotAvailable
        }
    );
    assert_eq!(
        coverage
            .confirmation_current(HostId::new(), &evidence, T0)
            .await,
        Err(UnverifiableReason::NotAvailable)
    );
    let fake = domovoi_testkit::FakeCoverage::covering(&[COVERED]);
    assert_eq!(
        fake.confirmation_current(HostId::new(), &evidence, T0).await,
        Err(UnverifiableReason::NotAvailable),
        "a fake that is not told otherwise fails closed too"
    );
}

#[tokio::test]
async fn owner_cannot_confirm_uncovered_management_path() {
    let rig = rig();
    rig.verified(1, 0);
    let (service, _) = rig.service();
    let path = rig.path(&service, OUTSIDE).await;
    assert_eq!(
        rig.confirm(&service, &path).await.unwrap_err(),
        KnowledgeError::PathUncovered
    );
}

#[tokio::test]
async fn new_management_path_requires_local_helper_configuration() {
    // A path outside the helper's configured networks stays unconfirmable until the
    // owner edits the helper's configuration on the host and a new attestation shows it.
    let rig = rig();
    rig.verified(1, 0);
    let (service, _) = rig.service();
    let path = rig.path(&service, "203.0.113.0/24").await;
    assert_eq!(
        rig.confirm(&service, &path).await.unwrap_err(),
        KnowledgeError::PathUncovered
    );
    let covered = rig.path(&service, COVERED).await;
    let view = rig.confirm(&service, &covered).await.unwrap();
    assert_eq!(view.state, PathState::Confirmed);
}

#[tokio::test]
async fn changed_helper_safety_set_invalidates_confirmation() {
    let rig = rig();
    rig.verified(1, 0);
    let (service, _) = rig.service();
    let path = rig.path(&service, COVERED).await;
    rig.confirm(&service, &path).await.unwrap();
    let listed = service.confirmed_paths_with_currency(rig.host).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].currency, Ok(()));
    // The helper's safety set changes: a new generation with another configuration.
    rig.verified(2, 1);
    let listed = service.confirmed_paths_with_currency(rig.host).await.unwrap();
    assert_eq!(
        listed[0].currency,
        Err(UnverifiableReason::ConfirmationStale)
    );
    // path_state itself is unchanged: the currency is a separate question.
    assert_eq!(listed[0].path.state, PathState::Confirmed);
}

#[tokio::test]
async fn stale_attestation_blocks_unattended_containment() {
    let rig = rig();
    rig.verified(1, 0);
    let (service, _) = rig.service();
    let path = rig.path(&service, COVERED).await;
    rig.confirm(&service, &path).await.unwrap();
    rig.clock.advance(SignedDuration::from_secs(300));
    let listed = service.confirmed_paths_with_currency(rig.host).await.unwrap();
    assert_eq!(
        listed[0].currency,
        Err(UnverifiableReason::AttestationStale)
    );
    let other = rig.path(&service, "192.0.2.128/25").await;
    assert_eq!(
        rig.confirm(&service, &other).await.unwrap_err(),
        KnowledgeError::CoverageUnverifiable(UnverifiableReason::AttestationStale)
    );
}

#[test]
fn every_unverified_reason_maps_to_a_coverage_reason() {
    for reason in UnverifiedReason::ALL {
        let back = UnverifiedReason::from(UnverifiableReason::from(reason));
        assert_eq!(back, reason);
    }
    let _ = Timestamp::UNIX_EPOCH;
}
