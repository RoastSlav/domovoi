//! C1-C3 (key exchange, enrollment, lifecycle) and D1-D2 (nonce).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{BOOT_A, BOOT_B, HOST, Rig, T0};
use r10::decision::{Decision, HubAuth, hub_decide};
use r10::hub::{KeyStatus, NONCE_TTL, NonceError, NonceStore, Outcome, PinError, Rejection};
use r10::keys::{b64, b64_fixed, fingerprint, generate_key, group4, key_id, public_bytes};
use r10::state::ActionError;

#[test]
fn helper_public_key_exchange_and_fingerprint() {
    let pk = public_bytes(&generate_key());
    let w = b64(&pk);
    assert_eq!(w.len(), 44);
    assert!(w.ends_with('='));
    assert_eq!(b64_fixed::<32>(&w).unwrap(), pk);
    for bad in [b64(&pk[..31]), b64(&[pk.to_vec(), vec![1]].concat()), w.trim_end_matches('=').to_owned(), format!("{w}\n"), String::new()] {
        assert!(b64_fixed::<32>(&bad).is_err(), "{bad:?}");
    }
    let fp = fingerprint(&pk);
    assert_eq!(fp.len(), 64);
    assert!(fp.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(fp, hex::encode(r10::hash::sha256(&pk)));
    assert_eq!(key_id(&pk), fp[..16]);
    assert_eq!(group4(&fp).split(' ').count(), 16);
    // The hub computes the fingerprint from the key bytes: Enrollment::observe takes no
    // fingerprint or id from the agent at all.
    let e = r10::hub::Enrollment::default();
    assert_eq!(e.observe(HOST, &pk), KeyStatus::Unverified { fingerprint: fp });
}

#[test]
fn first_contact_does_not_pin_key() {
    let mut rig = Rig::new();
    rig.hub.enroll.unpin(HOST); // back to a host that was never pinned
    let att = rig.attest_wire(T0, BOOT_A);
    assert!(matches!(rig.hub.enroll.observe(HOST, &rig.pk), KeyStatus::Unverified { .. }));
    assert_eq!(rig.hub.verify(HOST, &att, T0).unwrap_err(), Rejection::KeyNotPinned);
    assert!(rig.hub.enroll.pinned(HOST).is_none(), "receiving a key and verifying never pins");
}

#[test]
fn key_substitution_by_agent_is_detected() {
    let mut rig = Rig::new();
    let other = public_bytes(&generate_key());
    assert!(matches!(rig.hub.enroll.observe(HOST, &other), KeyStatus::Mismatch { .. }));
    // Owner enrollment needs step-up and the locally compared fingerprint.
    assert_eq!(rig.hub.enroll.pin(HOST, &other, &fingerprint(&other), false), Err(PinError::StepUp));
    assert_eq!(rig.hub.enroll.pin(HOST, &other, &fingerprint(&rig.pk), true), Err(PinError::FingerprintMismatch));
    assert_eq!(rig.hub.enroll.pinned(HOST), Some(rig.pk), "the old pin is kept");
    assert_eq!(rig.hub.enroll.audit, vec!["helper.key_pinned"]);
}

#[test]
fn helper_state_recovery_requires_explicit_local_action() {
    let mut rig = Rig::new();
    let (sp, kp) = (rig.state_path(), rig.dir.0.join("attest.key"));
    let reason = |rig: &Rig| {
        let bytes = rig.attest_raw([1; 32], T0, BOOT_A).map(|a| a.report);
        bytes.map(|b| String::from_utf8(b).unwrap())
    };
    // state_missing: key present, state absent. Stays that way: nothing resets it.
    fs::remove_file(&sp).unwrap();
    for _ in 0..2 {
        let r = reason(&rig).unwrap();
        assert!(r.contains("complete=false\nincomplete_reason=state_missing\n"), "{r}");
        assert!(r.contains("generation=0\n"));
        assert!(!sp.exists(), "no silent state creation");
    }
    // init refuses while the key exists.
    assert!(matches!(rig.dir.init(None, BOOT_A), Err(ActionError::Exists)));
    // recover: retire, new key, generation 1.
    let old_pk = rig.pk;
    rig.pk = rig.dir.recover("20260101T000000Z", rig.hashes(), BOOT_A).unwrap();
    assert_ne!(rig.pk, old_pk);
    assert!(rig.dir.0.join("retired/20260101T000000Z/attest.key").exists());
    assert!(reason(&rig).unwrap().contains("complete=true\n") );
    let st = fs::read_to_string(&sp).unwrap();
    assert!(st.contains("generation=1\n"));
    // key_missing and uninitialized cannot be signed at all.
    fs::remove_file(&kp).unwrap();
    assert!(matches!(rig.attest_raw([1; 32], T0, BOOT_A), Err(r10::helper::Unsigned(r10::state::FailClosed::KeyMissing))));
    assert!(!kp.exists(), "the helper never generates a key on its own");
    fs::remove_file(&sp).unwrap();
    assert!(matches!(rig.attest_raw([1; 32], T0, BOOT_A), Err(r10::helper::Unsigned(r10::state::FailClosed::Uninitialized))));
    assert!(!kp.exists() && !sp.exists());
    // Only init (neither exists) creates again.
    assert!(rig.dir.init(None, BOOT_A).is_ok());
    // recover/rotate with nothing to act on.
    let empty = r10::state::HelperDir(rig.tmp.path().join("other"));
    assert!(matches!(empty.recover("t", None, BOOT_A), Err(ActionError::NothingToRetire)));
    assert!(matches!(empty.rotate_key("t", None, BOOT_A), Err(ActionError::NothingToRetire)));
    let _ = &mut rig;
}

#[test]
fn key_rotation_blocks_until_reenrolled() {
    let mut rig = Rig::new();
    // Drive the old key's generation up first.
    let a = rig.attest_wire(T0, BOOT_A);
    assert!(matches!(rig.hub.verify(HOST, &a, T0), Ok(Outcome::Verified(_))));
    rig.toml.push_str("# comment only\n");
    rig.snap.routes.push(r10::fixture::route(254, "192.168.1.0/24", None, "eth0", 50));
    let b = rig.attest_wire(T0 + 1, BOOT_A);
    let Ok(Outcome::Verified(v)) = rig.hub.verify(HOST, &b, T0 + 1) else { panic!() };
    assert!(v.identity.generation >= 1);
    let old_pk = rig.pk;
    let new_pk = rig.dir.rotate_key("t1", rig.hashes(), BOOT_A).unwrap();
    rig.pk = new_pk;
    let c = rig.attest_wire(T0 + 2, BOOT_A);
    assert_eq!(rig.hub.verify(HOST, &c, T0 + 2).unwrap_err(), Rejection::KeyMismatch);
    assert_eq!(rig.hub.gate(HOST, T0 + 3).unwrap_err(), r10::safety::UnverifiedReason::KeyMismatch);
    assert!(matches!(
        hub_decide(HubAuth::Policy, rig.hub.gate(HOST, T0 + 3), &[], &[], T0 + 3),
        Decision::Refused(_)
    ));
    // Owner re-enrolment after the local fingerprint comparison.
    let fp = rig.dir.fingerprint().unwrap();
    rig.hub.enroll.pin(HOST, &new_pk, &fp, true).unwrap();
    let d = rig.attest_wire(T0 + 4, BOOT_A);
    let Ok(Outcome::Verified(v2)) = rig.hub.verify(HOST, &d, T0 + 4) else { panic!() };
    assert_eq!(v2.identity.generation, 1, "new key starts a new sequence; no generation_regressed");
    assert!(rig.hub.highest_generation(&old_pk).unwrap() >= 1);
    assert_eq!(rig.hub.enroll.audit, vec!["helper.key_pinned", "helper.key_rotated"]);
}

#[test]
fn key_rotation_invalidates_previous_pin() {
    let mut rig = Rig::new();
    let old_att = rig.attest_wire(T0, BOOT_A);
    let new_pk = rig.dir.rotate_key("t1", rig.hashes(), BOOT_A).unwrap();
    rig.hub.enroll.pin(HOST, &new_pk, &fingerprint(&new_pk), true).unwrap();
    // A message from the retired key no longer verifies, even with a fresh valid nonce state.
    assert_eq!(rig.hub.verify(HOST, &old_att, T0).unwrap_err(), Rejection::KeyMismatch);
    let mode = fs::metadata(rig.dir.0.join("retired/t1/attest.key")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "retired private key keeps 0600 (its retention is an open question)");
}

#[test]
fn unpin_blocks_containment() {
    let mut rig = Rig::new();
    let a = rig.attest_wire(T0, BOOT_A);
    assert!(matches!(rig.hub.verify(HOST, &a, T0), Ok(Outcome::Verified(_))));
    assert!(rig.hub.gate(HOST, T0 + 1).is_ok());
    rig.hub.enroll.unpin(HOST);
    assert_eq!(rig.hub.gate(HOST, T0 + 1).unwrap_err(), r10::safety::UnverifiedReason::KeyNotPinned);
    assert!(rig.hub.enroll.audit.contains(&"helper.key_unpinned".to_owned()));
}

#[test]
fn agent_reenrollment_keeps_helper_pin() {
    let mut rig = Rig::new();
    rig.hub.enroll.agent_reenroll(HOST);
    assert_eq!(rig.hub.enroll.pinned(HOST), Some(rig.pk));
    rig.hub.enroll.delete_host(HOST);
    assert_eq!(rig.hub.enroll.pinned(HOST), None, "deleting the host deletes the pin");
}

#[test]
fn init_with_sentinel_hashes_makes_the_first_attestation_generation_two() {
    // Ambiguity: init must write config_hash and route_state_hash but the contract does not say
    // from what. With computed hashes the first report is generation 1; with the sentinel it is 2.
    let gen_of = |computed: bool| {
        let rig = Rig::with_init(computed);
        let r = rig.attest_raw([3; 32], T0, BOOT_A).unwrap().report;
        String::from_utf8(r).unwrap().lines().find(|l| l.starts_with("generation=")).unwrap().to_owned()
    };
    assert_eq!(gen_of(true), "generation=1");
    assert_eq!(gen_of(false), "generation=2");
    let _ = BOOT_B;
}

#[test]
fn nonce_is_32_random_bytes_with_expected_properties() {
    let mut s = NonceStore::default();
    let ns: Vec<[u8; 32]> = (0..64).map(|_| s.issue(HOST, T0)).collect();
    let set: std::collections::BTreeSet<_> = ns.iter().collect();
    assert_eq!(set.len(), 64, "no repeats in 64 draws");
    assert!(ns.iter().all(|n| n.iter().any(|b| *b != n[0])), "not constant");
    let ones: u32 = ns.iter().flatten().map(|b| b.count_ones()).sum();
    assert!((8192 - 600..8192 + 600).contains(&ones), "bit balance {ones}/16384");
    assert_eq!(s.outstanding(), 64);
}

#[test]
fn nonce_lifetime_and_single_use_with_injected_clock() {
    let mut s = NonceStore::default();
    let n = s.issue(HOST, T0);
    assert_eq!(s.consume(HOST, &n, T0 + NONCE_TTL - 1), Ok(()));
    assert_eq!(s.consume(HOST, &n, T0 + 1), Err(NonceError::Unknown), "single use");
    let n = s.issue(HOST, T0);
    assert_eq!(s.consume(HOST, &n, T0 + NONCE_TTL), Err(NonceError::Expired), "60 s is already expired (stricter reading)");
    assert_eq!(s.outstanding(), 0);
}

#[test]
fn helper_attestation_rejects_replayed_nonce() {
    let mut rig = Rig::new();
    let a = rig.attest_wire(T0, BOOT_A);
    assert!(matches!(rig.hub.verify(HOST, &a, T0 + 1), Ok(Outcome::Verified(_))));
    assert_eq!(rig.hub.verify(HOST, &a, T0 + 2).unwrap_err(), Rejection::Nonce(NonceError::Unknown));
}

#[test]
fn attestation_rejected_for_other_host() {
    let mut rig = Rig::new();
    rig.hub.enroll.pin("host-2", &rig.pk, &fingerprint(&rig.pk), true).unwrap();
    let n = rig.hub.nonces.issue("host-2", T0);
    let att = common::wire(&rig.attest_raw(n, T0, BOOT_A).unwrap());
    assert_eq!(rig.hub.verify(HOST, &att, T0).unwrap_err(), Rejection::Nonce(NonceError::OtherHost));
    // Missing nonce: never issued at all.
    let att = common::wire(&rig.attest_raw([5; 32], T0, BOOT_A).unwrap());
    assert_eq!(rig.hub.verify(HOST, &att, T0).unwrap_err(), Rejection::Nonce(NonceError::Unknown));
}

#[test]
fn expired_attestation_nonce_is_rejected_and_an_unauthentic_message_does_not_burn_a_nonce() {
    let mut rig = Rig::new();
    let a = rig.attest_wire(T0, BOOT_A);
    assert_eq!(rig.hub.verify(HOST, &a, T0 + 61).unwrap_err(), Rejection::Nonce(NonceError::Expired));
    let b = rig.attest_wire(T0 + 100, BOOT_A);
    let mut sig = r10::keys::b64_decode(&b.signature).unwrap();
    sig[0] ^= 1;
    let bad = r10::hub::Attestation { signature: b64(&sig), ..b.clone() };
    assert!(rig.hub.verify(HOST, &bad, T0 + 101).is_err());
    assert_eq!(rig.hub.nonces.outstanding(), 1, "nonce still outstanding after a forged reply");
    assert!(matches!(rig.hub.verify(HOST, &b, T0 + 102), Ok(Outcome::Verified(_))));
}
