//! The unverified human path: every way the helper's safety can be unproven is a person's
//! decision, never a policy's; and what the person approved is stored and audited.

use domovoi_core::StepOutcome;
use domovoi_core::action::PlanAssurance;
use domovoi_core::attest::{
    Authority, Cidr, ExpectedSafety, LatestOutcome, TrustView, UnverifiedReason,
};
use domovoi_core::containment::keys;
use domovoi_core::ports::BlockPhase;
use domovoi_testkit::build::T0;
use jiff::{SignedDuration, Timestamp};

use super::world::{World, net, ok, rejected};
use crate::containment::input::{AssessError, Assessment, assess, containment_input};
use crate::containment::phases::{ContainEnd, execute_containment};

const TTL: i64 = 600;

fn later(secs: i64) -> Timestamp {
    T0.checked_add(SignedDuration::from_secs(secs)).unwrap()
}

async fn assessed(
    world: &World,
    authority: Authority,
    now: Timestamp,
) -> Result<Assessment, AssessError> {
    let deps = world.driver.deps();
    assess(&*deps.trust, &*deps.paths, world.host, authority, now).await
}

/// The person gets the unverified path with `reason`; a policy run is refused naming it.
async fn assert_unverified_for_human_only(world: &World, now: Timestamp, reason: UnverifiedReason) {
    let human = assessed(world, Authority::Human, now)
        .await
        .expect("a person may go on with unverified coverage");
    assert_eq!(human.reason(), Some(reason));
    assert!(matches!(human.assurance, PlanAssurance::Unverified(r) if r == reason));
    assert!(
        matches!(human.expected, ExpectedSafety::Unverified { reason: r } if r == reason),
        "the dispatched expected_safety carries the reason, never a made-up identity"
    );
    assert_eq!(
        assessed(world, Authority::Policy, now).await.unwrap_err(),
        AssessError::PolicyUnverified(reason),
        "no policy run goes on without a verified identity"
    );
}

#[tokio::test]
async fn human_containment_with_no_pinned_key_uses_unverified_path() {
    let world = World::new().await;
    assert_unverified_for_human_only(&world, T0, UnverifiedReason::KeyNotPinned).await;
}

#[tokio::test]
async fn human_containment_with_expired_attestation_uses_unverified_path() {
    let world = World::new().await;
    world.verify();
    assert_unverified_for_human_only(&world, later(400), UnverifiedReason::AttestationStale).await;
}

#[tokio::test]
async fn human_containment_with_invalid_signature_uses_unverified_path() {
    // A report whose signature does not verify is rejected and never stored: the host keeps its
    // pin and holds no verified state, so the gate reads it as a missing attestation.
    let world = World::new().await;
    world.verify();
    let mut view = world.trust_view().await;
    view.verified = None;
    view.latest = None;
    world.trust.set(world.host, view);
    assert_unverified_for_human_only(&world, T0, UnverifiedReason::AttestationMissing).await;
}

#[tokio::test]
async fn human_containment_with_incomplete_routes_uses_unverified_path() {
    let world = World::new().await;
    world.verify();
    let mut view = world.trust_view().await;
    view.latest = Some(LatestOutcome::SignedIncomplete);
    world.trust.set(world.host, view);
    assert_unverified_for_human_only(&world, T0, UnverifiedReason::ReportIncomplete).await;
}

type Break = fn(&mut TrustView);

#[tokio::test]
async fn unattended_containment_refused_in_every_unverified_case() {
    let world = World::new().await;
    // Each way the gate can fail, in turn, against a host that was verified a moment ago.
    let cases: [(UnverifiedReason, Timestamp, Break); 6] = [
        (UnverifiedReason::KeyNotPinned, T0, |v| v.pin = None),
        (UnverifiedReason::KeyMismatch, T0, |v| {
            v.latest = Some(LatestOutcome::Mismatch);
        }),
        (UnverifiedReason::AttestationMissing, T0, |v| {
            v.verified = None;
        }),
        (UnverifiedReason::AttestationStale, later(400), |_| {}),
        (UnverifiedReason::ReportIncomplete, T0, |v| {
            v.latest = Some(LatestOutcome::SignedIncomplete);
        }),
        (UnverifiedReason::GenerationRegressed, T0, |v| {
            v.latest = Some(LatestOutcome::Regressed);
        }),
    ];
    for (reason, now, break_it) in cases {
        world.verify();
        let mut view = world.trust_view().await;
        break_it(&mut view);
        world.trust.set(world.host, view);
        assert_eq!(
            assessed(&world, Authority::Policy, now).await.unwrap_err(),
            AssessError::PolicyUnverified(reason),
            "{reason:?}"
        );
    }
    // A verified host with a management path the report does not cover is the last case.
    world.verify();
    world
        .paths
        .set(vec![Cidr::host("203.0.113.9".parse().unwrap())]);
    assert_eq!(
        assessed(&world, Authority::Policy, T0).await.unwrap_err(),
        AssessError::PolicyUnverified(UnverifiedReason::PathUncovered)
    );
}

#[tokio::test]
async fn exclusion_overlap_refused_even_on_unverified_human_path() {
    let world = World::new().await;
    // No pinned key: the person goes the unverified way. The hub counts every target as
    // overlapping while nothing is verified, and the helper's own refusal ends the block.
    let view = world.trust_view().await;
    let input = containment_input(T0, &view, &net("198.51.100.0/24"), &[], false);
    assert!(!input.attested && input.target_overlaps_verified_set);
    world
        .relay
        .queue_result(rejected("exclusion_overlap: 192.0.2.0/24"));
    let (authorized, prepared, session, epoch) =
        world.step("192.0.2.0/25", TTL, Authority::Human).await;
    assert!(matches!(
        prepared.assessment.expected,
        ExpectedSafety::Unverified { .. }
    ));
    let id = authorized.execution().execution;
    let end =
        execute_containment(world.driver.deps(), &authorized, &prepared, session, epoch).await;
    assert!(matches!(
        end,
        ContainEnd::Settled {
            outcome: StepOutcome::Refused,
            ..
        }
    ));
    let row = world.block(id).await;
    assert_eq!(row.phase, BlockPhase::NotRun);
    assert_eq!(row.failure_token.as_deref(), Some("exclusion_overlap"));
    // Only the apply was sent: nothing was confirmed after the refusal.
    assert_eq!(world.relay.events(), ["dispatch:apply", "result"]);
}

/// The gate a policy-authorized dispatch asks before anything is sent: whether the owner's last
/// refresh showed the host's drills passed.
#[tokio::test]
async fn policy_dispatch_refused_without_passed_drills() {
    let world = World::new().await;
    let actor = || domovoi_core::Actor::System("tests".to_owned());
    // Never refreshed since this start: not passed, so a policy run is refused.
    assert!(!world.driver.drills_passed(world.host));
    for (drills, passed) in [
        ("not_run", false),
        ("failed", false),
        ("unknown", false),
        ("passed", true),
    ] {
        world
            .relay
            .queue_result(ok(&[(keys::OUT_GATE, "open"), (keys::OUT_DRILLS, drills)]));
        world.driver.refresh(world.host, actor()).await.unwrap();
        assert_eq!(world.driver.drills_passed(world.host), passed, "{drills}");
    }
    // A later refresh that no longer shows them passed takes the permission away again.
    world
        .relay
        .queue_result(ok(&[(keys::OUT_DRILLS, "failed")]));
    world.driver.refresh(world.host, actor()).await.unwrap();
    assert!(!world.driver.drills_passed(world.host));
    // Every refresh was a read: no block exists.
    assert!(
        world
            .driver
            .blocks(world.host, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn unverified_approval_persists_assurance_and_audits_it() {
    let world = World::new().await;
    world.verify();
    // One management path the verified report does not cover: the person is told so, and what
    // they were told is what gets stored.
    world
        .paths
        .set(vec![Cidr::host("203.0.113.9".parse().unwrap())]);
    world
        .relay
        .queue_result(ok(&[(keys::OUT_EXISTING_FLOWS, "n/a")]));
    world
        .relay
        .queue_result(ok(&[(keys::OUT_BRIDGE_GAP, "false")]));
    let (authorized, prepared, session, epoch) =
        world.step("198.51.100.0/24", TTL, Authority::Human).await;
    let id = authorized.execution().execution;
    execute_containment(world.driver.deps(), &authorized, &prepared, session, epoch).await;

    let row = world.block(id).await;
    let stored: serde_json::Value = serde_json::from_str(&row.assurance).unwrap();
    assert_eq!(stored["kind"], "unverified");
    assert_eq!(stored["reason"], "path_uncovered");
    assert_eq!(
        stored["unprotected_paths"],
        serde_json::json!(["203.0.113.9/32"])
    );
    for dispatch in world.relay.dispatches() {
        assert!(matches!(
            dispatch.expected_safety(),
            Some(ExpectedSafety::Unverified {
                reason: UnverifiedReason::PathUncovered
            })
        ));
    }
    let audits = world.audits().await;
    let approved: Vec<_> = audits
        .iter()
        .filter(|a| a.kind == "containment.unverified_approved")
        .collect();
    assert_eq!(approved.len(), 1, "one approval, one audit row");
    let text = approved[0].detail.to_string();
    assert!(
        text.contains("path_uncovered") && text.contains("198.51.100.0/24"),
        "{text}"
    );
    assert!(!text.contains("password") && !text.contains("signature"));
}
