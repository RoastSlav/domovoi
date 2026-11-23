//! B1-B4, T1 and K4: signing, strict verification, ordering, acceptance, forged agent data.

mod common;

use common::{BOOT_A, HOST, Rig, T0, wire};
use r10::decision::{HubAuth, hub_decide};
use r10::hub::{Attestation, NonceError, Outcome, Rejection};
use r10::keys::{b64, b64_decode, generate_key, key_id, public_bytes, sign_report, signing_bytes, verify_report};
use r10::report::{EmptyReasonPolicy, ParseError, parse_report};
use r10::safety::UnverifiedReason;

fn signed(rig: &Rig, now: u64) -> (r10::helper::SignedAttestation, [u8; 32]) {
    let mut rig_nonce = [0u8; 32];
    rig_nonce.copy_from_slice(&rand::random::<[u8; 32]>());
    let _ = now;
    (rig.attest_raw(rig_nonce, T0, BOOT_A).expect("signed"), rig_nonce)
}

#[test]
fn helper_attestation_signature_verifies() {
    let mut rig = Rig::new();
    let att = rig.attest_wire(T0, BOOT_A);
    let sig = b64_decode(&att.signature).unwrap();
    assert_eq!(sig.len(), 64);
    let report = b64_decode(&att.report).unwrap();
    assert!(signing_bytes(&report).starts_with(b"domovoi-safety-attest-v1\n"));
    assert!(matches!(rig.hub.verify(HOST, &att, T0 + 1), Ok(Outcome::Verified(_))));
    // Domain separation: the same key's signature over the bare report must not verify.
    let sig_arr: [u8; 64] = sig.try_into().unwrap();
    assert!(verify_report(&rig.pk, &report, &sig_arr).unwrap());
    let bare = ed25519_dalek::Signature::from_bytes(&sig_arr);
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&rig.pk).unwrap();
    assert!(vk.verify_strict(&report, &bare).is_err(), "signature is over prefix||report, not the bare report");
}

#[test]
fn key_id_never_used_for_verification() {
    let mut rig = Rig::new();
    // Key B claims key A's label and is presented in the public_key field.
    let other = generate_key();
    let n = rig.hub.nonces.issue(HOST, T0);
    let honest = rig.attest_raw(n, T0, BOOT_A).unwrap();
    let mut r = parse_report(&honest.report, EmptyReasonPolicy::EmptyIffComplete).unwrap();
    assert_eq!(r.helper_key_id, key_id(&rig.pk));
    let bytes = r.render();
    let forged = Attestation { public_key: b64(&public_bytes(&other)), report: b64(&bytes), signature: b64(&sign_report(&other, &bytes)) };
    assert_eq!(rig.hub.verify(HOST, &forged, T0 + 1).unwrap_err(), Rejection::KeyMismatch);
    // Conversely a wrong label under the pinned key and a valid signature is accepted: the
    // label is not a verification input (and the contract does not say the hub must compare it).
    r.helper_key_id = "0000000000000000".into();
    let bytes = r.render();
    let key = {
        // sign with the real helper key: reload it from the key file
        let seed: [u8; 32] = std::fs::read(rig.dir.0.join("attest.key")).unwrap().try_into().unwrap();
        ed25519_dalek::SigningKey::from_bytes(&seed)
    };
    let att = Attestation { public_key: b64(&rig.pk), report: b64(&bytes), signature: b64(&sign_report(&key, &bytes)) };
    assert!(matches!(rig.hub.verify(HOST, &att, T0 + 1), Ok(Outcome::Verified(_))));
}

#[test]
fn attestation_rejected_for_wrong_key() {
    let mut rig = Rig::new();
    let n = rig.hub.nonces.issue(HOST, T0);
    let honest = rig.attest_raw(n, T0, BOOT_A).unwrap();
    let other = generate_key();
    // Pinned public key claimed, signed with another key.
    let att = Attestation { public_key: b64(&rig.pk), report: b64(&honest.report), signature: b64(&sign_report(&other, &honest.report)) };
    assert_eq!(rig.hub.verify(HOST, &att, T0 + 1).unwrap_err(), Rejection::SignatureInvalid);
}

#[test]
fn helper_attestation_rejects_forged_report() {
    let mut rig = Rig::new();
    let n = rig.hub.nonces.issue(HOST, T0);
    let good = wire(&rig.attest_raw(n, T0, BOOT_A).unwrap());
    let mut rep = b64_decode(&good.report).unwrap();
    rep[0] ^= 1;
    let altered = Attestation { report: b64(&rep), ..good.clone() };
    assert_eq!(rig.hub.verify(HOST, &altered, T0 + 1).unwrap_err(), Rejection::SignatureInvalid);
    let mut sig = b64_decode(&good.signature).unwrap();
    sig[10] ^= 1;
    let altered = Attestation { signature: b64(&sig), ..good.clone() };
    assert_eq!(rig.hub.verify(HOST, &altered, T0 + 1).unwrap_err(), Rejection::SignatureInvalid);
    sig.truncate(63);
    let altered = Attestation { signature: b64(&sig), ..good.clone() };
    assert!(matches!(rig.hub.verify(HOST, &altered, T0 + 1), Err(Rejection::BadEncoding(_))));
    let altered = Attestation { signature: "!!!".into(), ..good.clone() };
    assert!(matches!(rig.hub.verify(HOST, &altered, T0 + 1), Err(Rejection::BadEncoding(_))));
    // Public key of 31 and 33 bytes, and unpadded base64.
    for k in [b64(&rig.pk[..31]), b64(&[rig.pk.to_vec(), vec![0]].concat()), b64(&rig.pk).trim_end_matches('=').to_owned()] {
        let altered = Attestation { public_key: k, ..good.clone() };
        assert!(matches!(rig.hub.verify(HOST, &altered, T0 + 1), Err(Rejection::BadEncoding(_))));
    }
    // None of the failures consumed the nonce: the untouched message still verifies.
    assert!(matches!(rig.hub.verify(HOST, &good, T0 + 2), Ok(Outcome::Verified(_))));
}

fn helper_key(rig: &Rig) -> ed25519_dalek::SigningKey {
    let seed: [u8; 32] = std::fs::read(rig.dir.0.join("attest.key")).unwrap().try_into().unwrap();
    ed25519_dalek::SigningKey::from_bytes(&seed)
}

#[test]
fn signature_is_checked_before_parse_and_parser_sees_the_received_bytes() {
    let mut rig = Rig::new();
    let key = helper_key(&rig);
    // (a) valid signature over an unparseable report: parse stage reached, on the same bytes.
    let junk = b"format=2\nnot a report\n".to_vec();
    let att = Attestation { public_key: b64(&rig.pk), report: b64(&junk), signature: b64(&sign_report(&key, &junk)) };
    let e = rig.hub.verify(HOST, &att, T0).unwrap_err();
    assert!(matches!(e, Rejection::Parse(ParseError::BadFormat(_))), "{e:?}");
    assert!(rig.hub.trace.contains(&"parse_report"));
    assert_eq!(rig.hub.parsed_input.as_deref(), Some(junk.as_slice()), "parser received the received bytes verbatim");
    // (b) invalid signature over a valid report: rejected at signature, parser never ran.
    let n = rig.hub.nonces.issue(HOST, T0);
    let good = rig.attest_raw(n, T0, BOOT_A).unwrap();
    let other = generate_key();
    let att = Attestation { public_key: b64(&rig.pk), report: b64(&good.report), signature: b64(&sign_report(&other, &good.report)) };
    assert_eq!(rig.hub.verify(HOST, &att, T0).unwrap_err(), Rejection::SignatureInvalid);
    assert_eq!(rig.hub.trace.last(), Some(&"verify_signature"));
    assert!(rig.hub.parsed_input.is_none());
    // (c) fails both: reported as a signature failure.
    let att = Attestation { public_key: b64(&rig.pk), report: b64(&junk), signature: b64(&sign_report(&other, &junk)) };
    assert_eq!(rig.hub.verify(HOST, &att, T0).unwrap_err(), Rejection::SignatureInvalid);
    assert!(rig.hub.parsed_input.is_none());
    // (d) a non-canonical report (extra trailing byte) under a valid signature is not
    // normalised into validity: it is rejected by the parser, and re-serialising is never tried.
    let mut padded = good.report.clone();
    padded.extend_from_slice(b"\n");
    let att = Attestation { public_key: b64(&rig.pk), report: b64(&padded), signature: b64(&sign_report(&key, &padded)) };
    assert!(matches!(rig.hub.verify(HOST, &att, T0), Err(Rejection::Parse(ParseError::BlankLine(_)))));
}

#[test]
fn signed_report_with_wrong_config_hash_is_rejected_by_recomputation() {
    let mut rig = Rig::new();
    let key = helper_key(&rig);
    let n = rig.hub.nonces.issue(HOST, T0);
    let good = rig.attest_raw(n, T0, BOOT_A).unwrap();
    let mut r = parse_report(&good.report, EmptyReasonPolicy::EmptyIffComplete).unwrap();
    r.config_hash[0] ^= 1;
    let bytes = r.render();
    let att = Attestation { public_key: b64(&rig.pk), report: b64(&bytes), signature: b64(&sign_report(&key, &bytes)) };
    assert_eq!(rig.hub.verify(HOST, &att, T0).unwrap_err(), Rejection::Parse(ParseError::HashMismatch("config_hash")));
}

fn broken(rig: &mut Rig) {
    rig.toml = "[safety]\nmanagement_cidrs = [\"192.168.1.7/99\"]\n".into();
}

#[test]
fn signed_incomplete_report_not_accepted_as_verified() {
    let mut rig = Rig::new();
    broken(&mut rig);
    let att = rig.attest_wire(T0, BOOT_A);
    let out = rig.hub.verify(HOST, &att, T0 + 1);
    // Signature and nonce checks passed...
    assert!(rig.hub.trace.contains(&"verify_signature") && rig.hub.trace.contains(&"check_nonce"));
    let Ok(Outcome::SignedIncomplete { reason }) = out else { panic!("{out:?}") };
    assert_eq!(reason.as_deref(), Some("config_invalid"));
    // ...and the nonce is spent, yet nothing was stored as verified state.
    assert_eq!(rig.hub.nonces.outstanding(), 0);
    assert!(rig.hub.stored(HOST).is_none());
    assert_eq!(rig.hub.highest_generation(&rig.pk), None);
    assert_eq!(rig.hub.gate(HOST, T0 + 1).unwrap_err(), UnverifiedReason::AttestationMissing);
    assert!(matches!(
        hub_decide(HubAuth::Policy, rig.hub.gate(HOST, T0 + 1), &[], &[], T0 + 1),
        r10::decision::Decision::Refused(_)
    ));
}

#[test]
fn hub_rejects_unverified_safety_report() {
    let mut rig = Rig::new();
    // Not pinned: a correctly signed complete report is still not safety state.
    rig.hub.enroll.unpin(HOST);
    let att = rig.attest_wire(T0, BOOT_A);
    assert_eq!(rig.hub.verify(HOST, &att, T0).unwrap_err(), Rejection::KeyNotPinned);
    assert!(rig.hub.stored(HOST).is_none());
    assert_eq!(rig.hub.gate(HOST, T0).unwrap_err(), UnverifiedReason::KeyNotPinned);
    let _ = signed;
}

#[test]
fn later_incomplete_report_does_not_revoke_earlier_verified_state() {
    // Finding: the contract says an incomplete report is not accepted, but does not say it
    // invalidates the previous verified state. In this harness it does not; the helper still
    // refuses at apply time (safety_unknown), the hub gate stays open until staleness.
    let mut rig = Rig::new();
    let a = rig.attest_wire(T0, BOOT_A);
    assert!(matches!(rig.hub.verify(HOST, &a, T0), Ok(Outcome::Verified(_))));
    broken(&mut rig);
    let b = rig.attest_wire(T0 + 30, BOOT_A);
    assert!(matches!(rig.hub.verify(HOST, &b, T0 + 30), Ok(Outcome::SignedIncomplete { .. })));
    assert!(rig.hub.gate(HOST, T0 + 31).is_ok(), "stale-but-fresh verified state still passes the hub gate");
}

#[test]
fn agent_cannot_fabricate_effective_safety_set() {
    let mut rig = Rig::new();
    let n = rig.hub.nonces.issue(HOST, T0);
    let honest = rig.attest_raw(n, T0, BOOT_A).unwrap();
    // The agent drops the exclusions from the helper's report and re-signs with its own key.
    let mut r = parse_report(&honest.report, EmptyReasonPolicy::EmptyIffComplete).unwrap();
    r.config.cidr.clear();
    r.config_hash = r.config.hash();
    let forged = r.render();
    let agent_key = generate_key();
    let as_helper = Attestation { public_key: b64(&rig.pk), report: b64(&forged), signature: b64(&sign_report(&agent_key, &forged)) };
    assert_eq!(rig.hub.verify(HOST, &as_helper, T0).unwrap_err(), Rejection::SignatureInvalid);
    let as_self = Attestation { public_key: b64(&public_bytes(&agent_key)), report: b64(&forged), signature: b64(&sign_report(&agent_key, &forged)) };
    assert_eq!(rig.hub.verify(HOST, &as_self, T0).unwrap_err(), Rejection::KeyMismatch);
    // Altering the relayed helper bytes while keeping the helper's signature also fails.
    let relayed = Attestation { public_key: b64(&rig.pk), report: b64(&forged), signature: b64(&honest.signature) };
    assert_eq!(rig.hub.verify(HOST, &relayed, T0).unwrap_err(), Rejection::SignatureInvalid);
    assert!(rig.hub.stored(HOST).is_none());
    let _ = NonceError::Unknown;
}
