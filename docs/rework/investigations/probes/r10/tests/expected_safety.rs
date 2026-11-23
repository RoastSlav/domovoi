//! J1-J2 (races), K1-K3 (expected_safety, adversarial agent), L1-L2 (unverified path),
//! P1 (coverage and confirmation), M1-M2 (limits, freshness).

mod common;

use common::{BOOT_A, BOOT_B, HOST, Rig, T0, env};
use r10::coverage::{Candidate, ConfirmError, Source, candidate, confirm, confirmation_current, covered};
use r10::decision::{Decision, HubAuth, agent_check, approval_card, hub_decide};
use r10::fixture::{FixtureSource, cidr};
use r10::helper::{AuthKind, ContainRequest, HelperRuntime, Inputs, Refused, apply_contain};
use r10::hub::{FRESH_FOR, Outcome, VerifiedState};
use r10::safety::{ExpectedSafety, SafetyIdentity, UnverifiedReason};
use serde_json::json;

fn verified(rig: &mut Rig, now: u64, boot: &str) -> VerifiedState {
    let a = rig.attest_wire(now, boot);
    match rig.hub.verify(HOST, &a, now) {
        Ok(Outcome::Verified(v)) => *v,
        other => panic!("{other:?}"),
    }
}

fn req(target: &str, expected: Option<ExpectedSafety>) -> ContainRequest {
    ContainRequest { target: target.into(), ttl_s: 60, expected, agent_max_ttl: None, agent_max_active: None, agent_reported_active: None }
}

fn apply(rig: &Rig, r: &ContainRequest, now: u64, boot: &str, rt: &mut HelperRuntime) -> Result<u64, Refused> {
    let src = FixtureSource(rig.snap.clone());
    let e = env(now, boot);
    apply_contain(&rig.dir, r, &Inputs { config_text: Some(&rig.toml), routes: &src, env: &e }, rt, &mut |_| {}, &mut |_| {})
}

fn rt() -> HelperRuntime {
    HelperRuntime { per_hour: 100, ..Default::default() }
}

fn exp(v: &VerifiedState) -> Option<ExpectedSafety> {
    Some(ExpectedSafety::Verified(v.identity.clone()))
}

const OK_TARGET: &str = "203.0.113.0/24";

#[test]
fn verified_identity_applies_when_nothing_changed_and_comment_edits_do_not_matter() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    rig.toml.push_str("# a comment\n\n");
    assert!(apply(&rig, &req(OK_TARGET, exp(&v)), T0 + 1, BOOT_A, &mut rt()).is_ok());
}

#[test]
fn config_change_between_attestation_and_apply_refused() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    rig.toml = rig.toml.replace("max_active = 5", "max_active = 6");
    assert_eq!(apply(&rig, &req(OK_TARGET, exp(&v)), T0 + 1, BOOT_A, &mut rt()), Err(Refused::SafetyChanged));
}

#[test]
fn network_change_between_attestation_and_apply_refused() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    rig.snap.routes.iter_mut().filter(|r| r.dst == cidr("192.168.1.0/24")).for_each(|r| r.metric = 9);
    assert_eq!(apply(&rig, &req(OK_TARGET, exp(&v)), T0 + 1, BOOT_A, &mut rt()), Err(Refused::SafetyChanged));
}

#[test]
fn reboot_between_permit_and_apply_refused() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    assert_eq!(apply(&rig, &req(OK_TARGET, exp(&v)), T0 + 1, BOOT_B, &mut rt()), Err(Refused::SafetyChanged));
}

#[test]
fn permit_from_before_reboot_refused() {
    // Same content, new boot: the permit's identity (with the old boot_id) is refused, while a
    // fresh identity taken after the reboot is accepted.
    let mut rig = Rig::new();
    let before = verified(&mut rig, T0, BOOT_A);
    let after = verified(&mut rig, T0 + 100, BOOT_B);
    assert_eq!(before.identity.generation, after.identity.generation);
    assert_ne!(before.identity.boot_id, after.identity.boot_id);
    assert_eq!(apply(&rig, &req(OK_TARGET, exp(&before)), T0 + 101, BOOT_B, &mut rt()), Err(Refused::SafetyChanged));
    assert!(apply(&rig, &req(OK_TARGET, exp(&after)), T0 + 101, BOOT_B, &mut rt()).is_ok());
}

#[test]
fn generation_change_between_permit_and_apply_refused() {
    // A change and its revert leave both hashes equal but advance the generation by two.
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    let orig = rig.snap.clone();
    rig.snap.routes.iter_mut().filter(|r| r.dst == cidr("192.168.1.0/24")).for_each(|r| r.metric = 9);
    let _ = rig.attest_raw([8; 32], T0 + 1, BOOT_A).unwrap();
    rig.snap = orig;
    let r = apply(&rig, &req(OK_TARGET, exp(&v)), T0 + 2, BOOT_A, &mut rt());
    assert_eq!(r, Err(Refused::SafetyChanged), "generation is part of the identity");
}

#[test]
fn helper_fails_closed_when_safety_state_unknown() {
    let rig = Rig::new();
    let src = FixtureSource(rig.snap.clone());
    let e = env(T0, BOOT_A);
    let r = req(OK_TARGET, None);
    let run = |text: Option<&str>, s: &FixtureSource| {
        apply_contain(&rig.dir, &r, &Inputs { config_text: text, routes: s, env: &e }, &mut rt(), &mut |_| {}, &mut |_| {})
    };
    assert_eq!(run(None, &src), Err(Refused::SafetyUnknown("config_unreadable".into())));
    assert_eq!(run(Some("[safety\n"), &src), Err(Refused::SafetyUnknown("config_invalid".into())));
    let mut failing = rig.snap.clone();
    failing.failures.lookups.push("192.168.1.10".parse().unwrap());
    assert_eq!(run(Some(&rig.toml), &FixtureSource(failing)), Err(Refused::SafetyUnknown("lookup_failed".into())));
}

#[test]
fn malformed_expected_safety_refused() {
    let id = SafetyIdentity { fingerprint: "ab".repeat(32), boot_id: BOOT_A.into(), generation: 3, config_hash: [1; 32], route_state_hash: [2; 32] };
    for e in [ExpectedSafety::Verified(id), ExpectedSafety::Unverified(UnverifiedReason::KeyNotPinned)] {
        assert_eq!(ExpectedSafety::parse(&e.to_json()).unwrap(), e, "both tagged forms parse");
    }
    let good = ExpectedSafety::Verified(SafetyIdentity { fingerprint: "ab".repeat(32), boot_id: BOOT_A.into(), generation: 3, config_hash: [1; 32], route_state_hash: [2; 32] }).to_json();
    let mut cases = vec![
        ("unknown_tag", json!({"state": "maybe"})),
        ("missing_tag", json!({"reason": "key_not_pinned"})),
        ("unverified_unknown_reason", json!({"state": "unverified", "reason": "because"})),
        ("unverified_extra_field", json!({"state": "unverified", "reason": "key_mismatch", "fingerprint": "x"})),
        ("unverified_missing_reason", json!({"state": "unverified"})),
        ("not_an_object", json!("verified")),
    ];
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("generation");
    cases.push(("verified_missing_field", missing));
    let mut extra = good.clone();
    extra["extra"] = json!(1);
    cases.push(("verified_extra_field", extra));
    let mut short = good.clone();
    short["config_hash"] = json!("abcd");
    cases.push(("verified_short_hash", short));
    let mut ty = good;
    ty["generation"] = json!("3");
    cases.push(("verified_generation_string", ty));
    for (name, v) in cases {
        let e = ExpectedSafety::parse(&v).expect_err(name);
        println!("MALFORMED {name}: {e}");
        assert!(agent_check(Some(&v), AuthKind::Human).is_err(), "{name}");
    }
    assert!(agent_check(None, AuthKind::Human).is_err(), "missing expected_safety on Contain is refused malformed_safety");
}

#[test]
fn agent_refuses_unverified_with_policy_label() {
    let u = ExpectedSafety::Unverified(UnverifiedReason::AttestationStale).to_json();
    assert_eq!(agent_check(Some(&u), AuthKind::Policy).unwrap(), Err("unverified_requires_human"));
    assert!(agent_check(Some(&u), AuthKind::Human).unwrap().is_ok());
}

#[test]
fn identity_comparison_covers_all_five_fields() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    let base = v.identity.clone();
    let muts: Vec<(&str, Box<dyn Fn(&mut SafetyIdentity)>)> = vec![
        ("fingerprint", Box::new(|i| i.fingerprint = "cd".repeat(32))),
        ("boot_id", Box::new(|i| i.boot_id = BOOT_B.into())),
        ("generation", Box::new(|i| i.generation += 1)),
        ("config_hash", Box::new(|i| i.config_hash[0] ^= 1)),
        ("route_state_hash", Box::new(|i| i.route_state_hash[0] ^= 1)),
    ];
    for (name, m) in muts {
        let mut i = base.clone();
        m(&mut i);
        let r = apply(&rig, &req(OK_TARGET, Some(ExpectedSafety::Verified(i))), T0 + 1, BOOT_A, &mut rt());
        assert_eq!(r, Err(Refused::SafetyChanged), "{name}");
    }
    assert!(apply(&rig, &req(OK_TARGET, Some(ExpectedSafety::Verified(base))), T0 + 1, BOOT_A, &mut rt()).is_ok());
}

#[test]
fn helper_refuses_target_overlapping_exclusion_even_if_agent_requests() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    for t in ["192.168.1.0/25", "192.168.0.0/16", "192.168.1.10/32", "2001:db8:1::/48", "0.0.0.0/0"] {
        let r = apply(&rig, &req(t, exp(&v)), T0 + 1, BOOT_A, &mut rt());
        assert_eq!(r, Err(Refused::ExclusionOverlap), "{t}");
    }
}

#[test]
fn helper_refuses_overlap_without_expected_safety() {
    let rig = Rig::new();
    assert_eq!(apply(&rig, &req("192.168.1.0/24", None), T0, BOOT_A, &mut rt()), Err(Refused::ExclusionOverlap));
}

#[test]
fn exclusion_overlap_refused_even_on_unverified_human_path() {
    let rig = Rig::new();
    for reason in UnverifiedReason::ALL {
        let r = apply(&rig, &req("192.168.1.0/24", Some(ExpectedSafety::Unverified(reason))), T0, BOOT_A, &mut rt());
        assert_eq!(r, Err(Refused::ExclusionOverlap), "{reason:?}");
    }
}

#[test]
fn compromised_agent_cannot_widen_helper_enforcement_but_can_omit_the_guard() {
    // Documented residual risk, demonstrated: omitting or marking `unverified` skips only the
    // identity comparison; a fabricated identity is refused; nothing lifts the actual limits.
    let rig = Rig::new();
    let fake = ExpectedSafety::Verified(SafetyIdentity { fingerprint: "00".repeat(32), boot_id: BOOT_A.into(), generation: 1, config_hash: [0; 32], route_state_hash: [0; 32] });
    assert_eq!(apply(&rig, &req(OK_TARGET, Some(fake)), T0, BOOT_A, &mut rt()), Err(Refused::SafetyChanged));
    assert!(apply(&rig, &req(OK_TARGET, None), T0, BOOT_A, &mut rt()).is_ok(), "omitting the guard still applies a non-overlapping target");
    let mut big = req("198.51.100.0/24", None);
    big.ttl_s = 999_999;
    big.agent_max_ttl = Some(u64::MAX);
    assert_eq!(apply(&rig, &big, T0, BOOT_A, &mut rt()), Err(Refused::TtlAboveMaximum));
}

fn paths() -> Vec<Candidate> {
    vec![candidate("192.168.1.0/24", Source::HubObserved).unwrap(), candidate("10.50.0.0/16", Source::AgentReported).unwrap()]
}

fn human() -> HubAuth {
    HubAuth::Human { step_up: true, typed_confirmation: true }
}

fn unverified_of(d: &Decision) -> UnverifiedReason {
    match d {
        Decision::UnverifiedHuman { reason, .. } => *reason,
        other => panic!("{other:?}"),
    }
}

#[test]
fn human_containment_with_no_pinned_key_uses_unverified_path() {
    let mut rig = Rig::new();
    let _ = verified(&mut rig, T0, BOOT_A);
    rig.hub.enroll.unpin(HOST);
    let d = hub_decide(human(), rig.hub.gate(HOST, T0 + 1), &paths(), &[], T0 + 1);
    assert_eq!(unverified_of(&d), UnverifiedReason::KeyNotPinned);
}

#[test]
fn human_containment_with_expired_attestation_uses_unverified_path() {
    let mut rig = Rig::new();
    let _ = verified(&mut rig, T0, BOOT_A);
    assert!(rig.hub.gate(HOST, T0 + FRESH_FOR - 1).is_ok());
    let d = hub_decide(human(), rig.hub.gate(HOST, T0 + FRESH_FOR), &paths(), &[], T0 + FRESH_FOR);
    assert_eq!(unverified_of(&d), UnverifiedReason::AttestationStale);
}

#[test]
fn human_containment_with_invalid_signature_uses_unverified_path() {
    let mut rig = Rig::new();
    let mut a = rig.attest_wire(T0, BOOT_A);
    a.signature = r10::keys::b64(&[0u8; 64]);
    let rej = rig.hub.verify(HOST, &a, T0).unwrap_err();
    let d = hub_decide(human(), Err(rej.unverified_reason()), &paths(), &[], T0);
    assert_eq!(unverified_of(&d), UnverifiedReason::SignatureInvalid);
}

#[test]
fn human_containment_with_incomplete_routes_uses_unverified_path() {
    let mut rig = Rig::new();
    rig.snap.failures.lookups.push("192.168.1.10".parse().unwrap());
    let a = rig.attest_wire(T0, BOOT_A);
    assert!(matches!(rig.hub.verify(HOST, &a, T0), Ok(Outcome::SignedIncomplete { .. })));
    let d = hub_decide(human(), Err(UnverifiedReason::ReportIncomplete), &paths(), &[], T0);
    assert_eq!(unverified_of(&d), UnverifiedReason::ReportIncomplete);
}

#[test]
fn unattended_containment_refused_in_every_unverified_case() {
    for r in UnverifiedReason::ALL {
        assert!(matches!(hub_decide(HubAuth::Policy, Err(r), &paths(), &[], T0), Decision::Refused(_)), "{r:?}");
    }
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    // Gate fine but a discovered path is uncovered (path_uncovered): still refused unattended.
    assert!(matches!(hub_decide(HubAuth::Policy, rig.hub.gate(HOST, T0 + 1), &paths(), &[], T0 + 1), Decision::Refused(_)));
    // Stale confirmation (confirmation_stale).
    let c = confirm(Ok(&v), &candidate("192.168.1.0/24", Source::Owner).unwrap(), T0).unwrap();
    rig.toml = rig.toml.replace("max_active = 5", "max_active = 6");
    let v2 = verified(&mut rig, T0 + 10, BOOT_A);
    assert!(matches!(hub_decide(HubAuth::Policy, rig.hub.gate(HOST, T0 + 11), &paths()[..1], std::slice::from_ref(&c), T0 + 11), Decision::Refused(_)));
    assert_eq!(confirmation_current(&c, Ok(&v2), T0 + 11), Err(UnverifiedReason::ConfirmationStale));
    // And the verified path is available once nothing is wrong.
    let c2 = confirm(Ok(&v2), &candidate("192.168.1.0/24", Source::Owner).unwrap(), T0 + 12).unwrap();
    assert!(matches!(hub_decide(HubAuth::Policy, rig.hub.gate(HOST, T0 + 13), &paths()[..1], &[c2], T0 + 13), Decision::Verified(ExpectedSafety::Verified(_))));
}

#[test]
fn human_approved_containment_shows_uncovered_paths() {
    let mut rig = Rig::new();
    let _ = verified(&mut rig, T0, BOOT_A);
    let d = hub_decide(human(), rig.hub.gate(HOST, T0 + 1), &paths(), &[], T0 + 1);
    let card = approval_card(&d);
    println!("{card}");
    assert!(card.contains("cannot be verified") && card.contains("path_uncovered"));
    assert!(card.contains("Not protected: 10.50.0.0/16") && !card.contains("Not protected: 192.168.1.0/24"));
    let Decision::UnverifiedHuman { audit, .. } = &d else { panic!() };
    assert_eq!(*audit, "containment.unverified_approved");
    // Without any verified data every discovered path is unverified and listed.
    rig.hub.enroll.unpin(HOST);
    let d = hub_decide(human(), rig.hub.gate(HOST, T0 + 1), &paths(), &[], T0 + 1);
    let card = approval_card(&d);
    assert!(card.contains("Not protected: 192.168.1.0/24") && card.contains("Not protected: 10.50.0.0/16"));
    // Step-up and typed confirmation are required (modelled as booleans only).
    assert!(matches!(hub_decide(HubAuth::Human { step_up: true, typed_confirmation: false }, rig.hub.gate(HOST, T0), &paths(), &[], T0), Decision::Refused(_)));
}

#[test]
fn unverified_value_never_carries_an_identity() {
    for r in UnverifiedReason::ALL {
        let j = ExpectedSafety::Unverified(r).to_json();
        let keys: Vec<&String> = j.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 2, "{j}");
        assert!(j.get("fingerprint").is_none() && j.get("generation").is_none());
    }
}

#[test]
fn owner_cannot_confirm_uncovered_management_path() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    for src in [Source::Owner, Source::AgentReported, Source::HubObserved] {
        let c = candidate("10.50.0.0/16", src).unwrap();
        assert_eq!(confirm(Ok(&v), &c, T0).unwrap_err(), ConfirmError::Uncovered);
    }
    let cov = |t: &str| covered(&v, &candidate(t, Source::Owner).unwrap());
    assert!(cov("192.168.1.64/26") && cov("192.168.1.0/24") && cov("2001:db8:5::/48"));
    assert!(cov("192.168.1.10/32"), "equals a hub_addr / link_peer address");
    assert!(!cov("192.168.0.0/23") && !cov("0.0.0.0/0") && !cov("2001:db9::/32"));
    assert_eq!(confirm(Err(UnverifiedReason::AttestationStale), &candidate("192.168.1.0/24", Source::Owner).unwrap(), T0).unwrap_err(), ConfirmError::Unverifiable(UnverifiedReason::AttestationStale));
}

#[test]
fn new_management_path_requires_local_helper_configuration() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    let c = candidate("10.50.0.0/16", Source::Owner).unwrap();
    assert!(confirm(Ok(&v), &c, T0).is_err());
    rig.toml = rig.toml.replace("\"2001:db8::/32\"", "\"2001:db8::/32\", \"10.50.0.0/16\"");
    rig.snap.routes.push(r10::fixture::route(254, "10.50.0.0/16", Some("192.168.1.1"), "eth0", 10));
    let v2 = verified(&mut rig, T0 + 5, BOOT_A);
    assert!(confirm(Ok(&v2), &c, T0 + 6).is_ok(), "covered only after the helper's own config lists it");
}

#[test]
fn changed_helper_safety_set_invalidates_confirmation() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    let cand = candidate("192.168.1.0/24", Source::Owner).unwrap();
    let c = confirm(Ok(&v), &cand, T0).unwrap();
    assert!(confirmation_current(&c, Ok(&v), T0 + 1).is_ok());
    rig.snap.routes.iter_mut().filter(|r| r.dst == cidr("192.168.1.0/24")).for_each(|r| r.metric = 3);
    let v2 = verified(&mut rig, T0 + 2, BOOT_A);
    assert_eq!(confirmation_current(&c, Ok(&v2), T0 + 3), Err(UnverifiedReason::ConfirmationStale));
}

#[test]
fn confirmation_survives_reboot_only_when_identity_otherwise_equal() {
    let mut rig = Rig::new();
    let v = verified(&mut rig, T0, BOOT_A);
    let c = confirm(Ok(&v), &candidate("192.168.1.0/24", Source::Owner).unwrap(), T0).unwrap();
    let rebooted = verified(&mut rig, T0 + 10, BOOT_B);
    assert_eq!(confirmation_current(&c, Ok(&rebooted), T0 + 11), Ok(()), "reboot exception for confirmations");
    // Reboot with changed content: not equal otherwise, stale.
    rig.snap.routes.iter_mut().filter(|r| r.dst == cidr("192.168.1.0/24")).for_each(|r| r.metric = 4);
    let changed = verified(&mut rig, T0 + 20, "cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    assert_eq!(confirmation_current(&c, Ok(&changed), T0 + 21), Err(UnverifiedReason::ConfirmationStale));
    // Age limit: 30 days.
    assert_eq!(confirmation_current(&c, Ok(&rebooted), T0 + 30 * 86_400), Err(UnverifiedReason::ConfirmationStale));
    // Reboot exception needs a fresh verified attestation.
    assert_eq!(confirmation_current(&c, Err(UnverifiedReason::AttestationStale), T0 + 11), Err(UnverifiedReason::AttestationStale));
}

#[test]
fn stale_attestation_blocks_unattended_containment() {
    let mut rig = Rig::new();
    let _ = verified(&mut rig, T0, BOOT_A);
    let pol = |t| hub_decide(HubAuth::Policy, rig.hub.gate(HOST, t), &[], &[], t);
    assert!(matches!(pol(T0 + 299), Decision::Verified(_)));
    assert!(matches!(pol(T0 + 300), Decision::Refused(_)));
}

#[test]
fn agent_limit_stricter_than_helper_applies() {
    let rig = Rig::new();
    let mut r = req(OK_TARGET, None);
    r.agent_max_ttl = Some(600);
    r.ttl_s = 900;
    assert_eq!(apply(&rig, &r, T0, BOOT_A, &mut rt()), Err(Refused::TtlAboveMaximum));
    r.ttl_s = 600;
    assert!(apply(&rig, &r, T0, BOOT_A, &mut rt()).is_ok());
}

#[test]
fn agent_limit_above_helper_is_capped() {
    let rig = Rig::new();
    let mut r = req(OK_TARGET, None);
    r.agent_max_ttl = Some(7200);
    r.ttl_s = 5000;
    assert_eq!(apply(&rig, &r, T0, BOOT_A, &mut rt()), Err(Refused::TtlAboveMaximum), "helper max 3600 wins");
    r.ttl_s = 3600;
    assert!(apply(&rig, &r, T0, BOOT_A, &mut rt()).is_ok());
}

#[test]
fn helper_active_count_is_authoritative() {
    let rig = Rig::new();
    let mut state = rt();
    state.nft = (0..5).map(|i| format!("198.51.100.{i}/32")).collect();
    let mut r = req(OK_TARGET, None);
    r.agent_reported_active = Some(0);
    r.agent_max_active = Some(100);
    assert_eq!(apply(&rig, &r, T0, BOOT_A, &mut state), Err(Refused::MaxActiveReached));
    state.nft = state.nft.iter().take(2).cloned().collect();
    r.agent_max_active = Some(2);
    assert_eq!(apply(&rig, &r, T0, BOOT_A, &mut state), Err(Refused::MaxActiveReached), "stricter agent limit applies");
    r.agent_max_active = None;
    assert!(apply(&rig, &r, T0, BOOT_A, &mut state).is_ok());
}

#[test]
fn mutation_limit_and_defaults_are_enforced() {
    let rig = Rig::new();
    let mut state = HelperRuntime { per_hour: 2, ..Default::default() };
    assert!(apply(&rig, &req("198.51.100.0/24", None), T0, BOOT_A, &mut state).is_ok());
    assert!(apply(&rig, &req("198.51.101.0/24", None), T0 + 1, BOOT_A, &mut state).is_ok());
    assert_eq!(apply(&rig, &req("198.51.102.0/24", None), T0 + 2, BOOT_A, &mut state), Err(Refused::MutationLimit));
    assert!(apply(&rig, &req("198.51.102.0/24", None), T0 + 3600, BOOT_A, &mut state).is_ok());
    // Defaults (C4-like): no cidr refuses; cidr but max_active 0 refuses.
    let mut empty = Rig::new();
    empty.toml = "[safety]\n".into();
    assert_eq!(apply(&empty, &req(OK_TARGET, None), T0, BOOT_A, &mut rt()), Err(Refused::NoManagementCidrs));
    empty.toml = "[safety]\nmanagement_cidrs=[\"192.168.1.0/24\"]\n".into();
    assert_eq!(apply(&empty, &req(OK_TARGET, None), T0, BOOT_A, &mut rt()), Err(Refused::MaxActiveReached));
}
