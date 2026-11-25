//! The latest-outcome gate, coverage, confirmation currency and the decision functions.

use domovoi_core::attest::{
    Authority, BootId, CONFIRMATION_MAX_AGE, Cidr, Decision, FRESHNESS, LatestOutcome, PinnedKey,
    TrustView, UnverifiedReason, VerifiedState, confirmation_current, covered, decide, gate,
    unprotected_paths,
};
use jiff::{SignedDuration, Timestamp};

use crate::common::{BOOT_B, cidr, ip};
use crate::identity::identity;

const KEY: [u8; 32] = [7; 32];
const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);

fn later(seconds: i64) -> Timestamp {
    T0 + SignedDuration::from_secs(seconds)
}

fn state(issued_at: Timestamp) -> VerifiedState {
    VerifiedState {
        identity: identity(),
        report: b"bytes".to_vec(),
        nonce_issued_at: issued_at,
        // Deliberately later than the issue time: freshness must not read this.
        accepted_at: issued_at + SignedDuration::from_secs(200),
        max_ttl_s: 3600,
        max_active: 2,
        cidrs: vec![cidr("192.0.2.0/24"), cidr("2001:db8::/32")],
        hub_addrs: vec![ip("198.51.100.10")],
        link_peers: vec![(ip("198.51.100.20"), 443)],
    }
}

fn view() -> TrustView {
    TrustView {
        pin: Some(PinnedKey::new(KEY, T0, "owner")),
        received_key: Some(KEY),
        latest: Some(LatestOutcome::Verified),
        verified: Some(state(T0)),
    }
}

fn gate_reason(view: &TrustView, now: Timestamp) -> Option<UnverifiedReason> {
    gate(now, view).err()
}

#[test]
fn gate_passes_a_pinned_verified_fresh_state() {
    let view = view();
    let state = gate(later(10), &view).expect("verified");
    assert_eq!(state.identity, identity());
}

#[test]
fn gate_reasons_follow_the_documented_order() {
    let mut v = view();
    v.pin = None;
    v.latest = Some(LatestOutcome::Mismatch);
    v.received_key = Some([9; 32]);
    assert_eq!(gate_reason(&v, T0), Some(UnverifiedReason::KeyNotPinned));

    let mut v = view();
    v.received_key = Some([9; 32]);
    v.latest = Some(LatestOutcome::SignedIncomplete);
    assert_eq!(gate_reason(&v, T0), Some(UnverifiedReason::KeyMismatch));

    let mut v = view();
    v.latest = Some(LatestOutcome::Mismatch);
    assert_eq!(gate_reason(&v, T0), Some(UnverifiedReason::KeyMismatch));

    let mut v = view();
    v.latest = Some(LatestOutcome::SignedIncomplete);
    assert_eq!(
        gate_reason(&v, T0),
        Some(UnverifiedReason::ReportIncomplete)
    );

    let mut v = view();
    v.latest = Some(LatestOutcome::Regressed);
    assert_eq!(
        gate_reason(&v, T0),
        Some(UnverifiedReason::GenerationRegressed)
    );

    let mut v = view();
    v.verified = None;
    assert_eq!(
        gate_reason(&v, T0),
        Some(UnverifiedReason::AttestationMissing)
    );
    v.latest = None;
    assert_eq!(
        gate_reason(&v, T0),
        Some(UnverifiedReason::AttestationMissing)
    );

    assert_eq!(
        gate_reason(&view(), later(300)),
        Some(UnverifiedReason::AttestationStale)
    );
}

#[test]
fn freshness_measured_from_nonce_issue_time() {
    let v = view();
    assert!(
        v.verified
            .as_ref()
            .is_some_and(|s| s.accepted_at > s.nonce_issued_at)
    );
    assert!(gate(later(299), &v).is_ok());
    assert_eq!(
        gate_reason(&v, later(300)),
        Some(UnverifiedReason::AttestationStale)
    );
    assert_eq!(
        gate_reason(&v, later(301)),
        Some(UnverifiedReason::AttestationStale)
    );
    assert_eq!(FRESHNESS, SignedDuration::from_secs(300));
    // A clock that is behind the issue time never reads as stale by itself.
    assert!(gate(later(-5), &v).is_ok());
}

#[test]
fn gate_follows_the_latest_outcome_with_no_sticky_flag() {
    let mut v = view();
    v.latest = Some(LatestOutcome::Mismatch);
    v.verified = None;
    assert_eq!(gate_reason(&v, T0), Some(UnverifiedReason::KeyMismatch));
    // One later good attestation restores trust: nothing else was remembered.
    v.latest = Some(LatestOutcome::Verified);
    v.verified = Some(state(later(30)));
    assert!(gate(later(40), &v).is_ok());
    // A key the session no longer reports is not a mismatch.
    v.received_key = None;
    assert!(gate(later(40), &v).is_ok());
}

#[test]
fn computed_at_not_used_for_trust() {
    use crate::common::{COMPUTED_AT, complete_report};
    let a = complete_report();
    let mut b = complete_report();
    b.computed_at = COMPUTED_AT + SignedDuration::from_secs(86_400 * 365);
    assert_eq!(a.config_hash, b.config.hash());
    assert_eq!(a.config.hash(), b.config.hash());
    assert_eq!(a.routes.hash(), b.routes.hash());
    // A VerifiedState has no computed_at: the gate cannot read it.
    let v = view();
    assert_eq!(gate_reason(&v, later(10)), None);
}

#[test]
fn covered_uses_only_the_verified_sets() {
    let s = state(T0);
    for inside in [
        "192.0.2.0/24",
        "192.0.2.128/25",
        "192.0.2.9/32",
        "2001:db8:1::/48",
        "198.51.100.10/32",
        "198.51.100.20/32",
    ] {
        assert!(covered(&s, &cidr(inside)), "{inside}");
    }
    for outside in [
        "192.0.0.0/16",
        "0.0.0.0/0",
        "198.51.100.0/24",
        "198.51.100.11/32",
        "203.0.113.1/32",
        "2001:db9::/32",
        "::/0",
    ] {
        assert!(!covered(&s, &cidr(outside)), "{outside}");
    }
}

fn identity_with(
    change: impl FnOnce(&mut domovoi_core::attest::SafetyIdentity),
) -> domovoi_core::attest::SafetyIdentity {
    let mut id = identity();
    change(&mut id);
    id
}

#[test]
fn confirmation_survives_reboot_only_when_identity_otherwise_equal() {
    let current = state(T0);
    let rebooted =
        |s: &mut VerifiedState| s.identity.boot_id = BootId::parse(BOOT_B).expect("boot");
    // Exact match.
    assert_eq!(
        confirmation_current(&identity(), T0, &current, later(60)),
        Ok(())
    );
    // Only the boot differs, and the current state is fresh.
    let mut after_reboot = state(later(60));
    rebooted(&mut after_reboot);
    assert_eq!(
        confirmation_current(&identity(), T0, &after_reboot, later(100)),
        Ok(())
    );
    // Only the boot differs but the current state is no longer fresh.
    let mut stale = state(T0);
    rebooted(&mut stale);
    assert_eq!(
        confirmation_current(&identity(), T0, &stale, later(300)),
        Err(UnverifiedReason::ConfirmationStale)
    );
    // The boot differs and so does something else.
    for change in [
        identity_with(|i| i.generation += 1),
        identity_with(|i| i.config_hash = domovoi_core::attest::ZERO_HASH),
        identity_with(|i| i.route_state_hash = domovoi_core::attest::ZERO_HASH),
        identity_with(|i| i.fingerprint = domovoi_core::KeyFingerprint::of_public_key(&[8; 32])),
    ] {
        assert_eq!(
            confirmation_current(&change, T0, &after_reboot, later(100)),
            Err(UnverifiedReason::ConfirmationStale)
        );
    }
}

#[test]
fn confirmation_current_rejects_changed_sets_and_old_confirmations() {
    let mut current = state(T0);
    current.identity.generation += 1;
    assert_eq!(
        confirmation_current(&identity(), T0, &current, later(10)),
        Err(UnverifiedReason::ConfirmationStale)
    );
    let current = state(later(CONFIRMATION_MAX_AGE.as_secs() - 10));
    let almost = later(CONFIRMATION_MAX_AGE.as_secs() - 1);
    assert_eq!(
        confirmation_current(&identity(), T0, &current, almost),
        Ok(())
    );
    let current = state(later(CONFIRMATION_MAX_AGE.as_secs()));
    assert_eq!(
        confirmation_current(
            &identity(),
            T0,
            &current,
            later(CONFIRMATION_MAX_AGE.as_secs())
        ),
        Err(UnverifiedReason::ConfirmationStale),
        "the age limit is strict"
    );
}

fn candidates() -> Vec<Cidr> {
    vec![
        cidr("192.0.2.0/25"),
        cidr("203.0.113.0/24"),
        cidr("198.51.100.10/32"),
    ]
}

#[test]
fn unprotected_paths_lists_every_uncovered_candidate() {
    let s = state(T0);
    assert_eq!(
        unprotected_paths(Ok(&s), &candidates()),
        vec![cidr("203.0.113.0/24")]
    );
    assert!(unprotected_paths(Ok(&s), &[]).is_empty());
    assert_eq!(
        unprotected_paths(Err(UnverifiedReason::KeyNotPinned), &candidates()),
        candidates(),
        "a failed gate protects nothing"
    );
}

fn unverified_views() -> Vec<(TrustView, Timestamp, UnverifiedReason)> {
    let mut out = Vec::new();
    let mut v = view();
    v.pin = None;
    out.push((v, T0, UnverifiedReason::KeyNotPinned));
    let mut v = view();
    v.received_key = Some([1; 32]);
    out.push((v, T0, UnverifiedReason::KeyMismatch));
    let mut v = view();
    v.latest = Some(LatestOutcome::SignedIncomplete);
    out.push((v, T0, UnverifiedReason::ReportIncomplete));
    let mut v = view();
    v.latest = Some(LatestOutcome::Regressed);
    out.push((v, T0, UnverifiedReason::GenerationRegressed));
    let mut v = view();
    v.verified = None;
    out.push((v, T0, UnverifiedReason::AttestationMissing));
    out.push((view(), later(400), UnverifiedReason::AttestationStale));
    out
}

#[test]
fn decide_gives_a_reason_for_every_unverified_case() {
    for (view, now, reason) in unverified_views() {
        assert_eq!(
            decide(now, &view, Authority::Human, &candidates()),
            Decision::UnverifiedHuman {
                reason,
                unprotected: candidates()
            },
            "{reason:?}"
        );
    }
    // A verified report that leaves a candidate uncovered is path_uncovered.
    assert_eq!(
        decide(later(5), &view(), Authority::Human, &candidates()),
        Decision::UnverifiedHuman {
            reason: UnverifiedReason::PathUncovered,
            unprotected: vec![cidr("203.0.113.0/24")]
        }
    );
}

#[test]
fn decide_refuses_policy_authority_in_every_unverified_case() {
    for (view, now, reason) in unverified_views() {
        assert_eq!(
            decide(now, &view, Authority::Policy, &candidates()),
            Decision::Refused(reason)
        );
    }
    assert_eq!(
        decide(later(5), &view(), Authority::Policy, &candidates()),
        Decision::Refused(UnverifiedReason::PathUncovered)
    );
}

#[test]
fn decide_verifies_when_the_gate_passes_and_every_candidate_is_covered() {
    let covered_only = vec![cidr("192.0.2.0/25"), cidr("198.51.100.10/32")];
    for authority in [Authority::Human, Authority::Policy] {
        assert_eq!(
            decide(later(5), &view(), authority, &covered_only),
            Decision::Verified(identity())
        );
        assert_eq!(
            decide(later(5), &view(), authority, &[]),
            Decision::Verified(identity())
        );
    }
}
