//! The hub's decisions before anything is sent: assessment, overlap, input for the evaluator.

use domovoi_core::action::PlanAssurance;
use domovoi_core::attest::{Authority, Cidr, UnverifiedReason};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

use super::world::{World, net};
use crate::containment::input::{
    AssessError, assess, containment_input, expected_from_assurance, overlaps_verified_set,
};

#[tokio::test]
async fn policy_assessment_is_refused_whenever_the_helper_is_unverified() {
    let world = World::new().await;
    let deps = world.driver.deps();
    let refused = assess(
        &*deps.trust,
        &*deps.paths,
        world.host,
        Authority::Policy,
        T0,
    )
    .await;
    assert_eq!(
        refused.unwrap_err(),
        AssessError::PolicyUnverified(UnverifiedReason::KeyNotPinned)
    );
    // A person gets the unverified path instead, with the reason.
    let human = assess(&*deps.trust, &*deps.paths, world.host, Authority::Human, T0)
        .await
        .unwrap();
    assert_eq!(human.reason(), Some(UnverifiedReason::KeyNotPinned));
    assert!(matches!(human.assurance, PlanAssurance::Unverified(_)));
}

#[tokio::test]
async fn an_expired_attestation_is_the_unverified_path_for_a_person() {
    let world = World::new().await;
    world.verify();
    let deps = world.driver.deps();
    let late = T0.checked_add(SignedDuration::from_secs(400)).unwrap();
    let human = assess(
        &*deps.trust,
        &*deps.paths,
        world.host,
        Authority::Human,
        late,
    )
    .await
    .unwrap();
    assert_eq!(human.reason(), Some(UnverifiedReason::AttestationStale));
    let policy = assess(
        &*deps.trust,
        &*deps.paths,
        world.host,
        Authority::Policy,
        late,
    )
    .await;
    assert!(policy.is_err());
}

#[tokio::test]
async fn an_uncovered_path_is_listed_as_not_protected() {
    let world = World::new().await;
    world.verify();
    world
        .paths
        .set(vec![Cidr::host("203.0.113.9".parse().unwrap())]);
    let deps = world.driver.deps();
    let human = assess(&*deps.trust, &*deps.paths, world.host, Authority::Human, T0)
        .await
        .unwrap();
    assert_eq!(human.reason(), Some(UnverifiedReason::PathUncovered));
    assert_eq!(human.unprotected.len(), 1);
}

#[tokio::test]
async fn containment_input_overlap_uses_verified_report_lines_only() {
    let world = World::new().await;
    world.verify();
    let view = world.trust_view().await;
    let state = view.verified.as_ref().unwrap();
    // The management network and the hub address come from the verified report.
    assert!(overlaps_verified_set(&net("192.0.2.0/25"), state));
    assert!(overlaps_verified_set(&net("192.0.2.10/32"), state));
    // A wider prefix that contains a verified line overlaps in the other direction.
    assert!(overlaps_verified_set(&net("192.0.0.0/16"), state));
    assert!(!overlaps_verified_set(&net("198.51.100.0/24"), state));
    assert!(overlaps_verified_set(&net("127.0.0.1/32"), state));
    let input = containment_input(T0, &view, &net("192.0.2.0/25"), &[], false);
    assert!(input.attested && input.target_overlaps_verified_set);
    let clear = containment_input(T0, &view, &net("198.51.100.0/24"), &[], false);
    assert!(!clear.target_overlaps_verified_set);
}

#[tokio::test]
async fn an_unattested_host_counts_every_target_as_overlapping() {
    let world = World::new().await;
    let view = world.trust_view().await;
    let input = containment_input(T0, &view, &net("198.51.100.0/24"), &[], false);
    assert!(!input.attested);
    assert!(input.target_overlaps_verified_set, "fail closed");
}

#[tokio::test]
async fn the_stored_assurance_rebuilds_the_expected_safety() {
    let world = World::new().await;
    world.verify();
    let deps = world.driver.deps();
    let verified = assess(&*deps.trust, &*deps.paths, world.host, Authority::Human, T0)
        .await
        .unwrap();
    assert!(verified.is_verified());
    assert!(expected_from_assurance("not json").is_none());
    assert!(
        expected_from_assurance(r#"{"kind":"unverified","reason":"key_not_pinned"}"#).is_some()
    );
    assert!(expected_from_assurance(r#"{"kind":"verified","identity":"nope"}"#).is_none());
}
