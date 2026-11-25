//! The verifier, step by step: every refusal, the order of the checks, and when the nonce is
//! consumed.

use domovoi_core::HostId;
use domovoi_core::attest::{
    FRESHNESS, LatestOutcome, NONCE_TTL, Nonce32, PinnedKey, UnverifiedReason, gate,
    render_complete,
};
use domovoi_core::ports::{AttestationReply, HighWater};
use domovoi_testkit::helper_attest::{TestHelper, complete_report, verified_view};
use jiff::{SignedDuration, Timestamp};

use super::nonce::{NonceError, NonceStore};
use super::verify::{Received, Verdict, verify};

fn t0() -> Timestamp {
    Timestamp::constant(1_700_000_000, 0)
}

struct Rig {
    helper: TestHelper,
    pin: PinnedKey,
    nonces: NonceStore,
    host: HostId,
}

fn rig() -> Rig {
    let helper = TestHelper::new(1);
    Rig {
        pin: PinnedKey::new(helper.public_key(), t0(), "user:test"),
        helper,
        nonces: NonceStore::new(),
        host: HostId::new(),
    }
}

impl Rig {
    fn nonce(&self) -> Nonce32 {
        self.nonces.issue(self.host, t0())
    }

    fn run(&self, reply: &AttestationReply, high_water: Option<&HighWater>) -> Verdict {
        self.run_at(reply, high_water, t0(), Some(&self.pin), self.host)
    }

    fn run_at(
        &self,
        reply: &AttestationReply,
        high_water: Option<&HighWater>,
        now: Timestamp,
        pin: Option<&PinnedKey>,
        host: HostId,
    ) -> Verdict {
        let AttestationReply::Signed {
            public_key,
            report,
            signature,
        } = reply
        else {
            panic!("a signed reply is needed");
        };
        verify(
            host,
            pin,
            &Received {
                public_key,
                report,
                signature,
            },
            &self.nonces,
            high_water,
            now,
        )
    }

    fn still_owed(&self, nonce: &Nonce32) -> bool {
        // Taking consumes it, so this is the last use of the nonce in a test.
        self.nonces.take(self.host, nonce, t0()).is_ok()
    }
}

fn rejected(verdict: &Verdict) -> (UnverifiedReason, &str) {
    match verdict {
        Verdict::Rejected { reason, detail } => (*reason, detail),
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn water(generation: u64, variant: u64) -> HighWater {
    let report = complete_report(Nonce32::from_bytes([0; 32]), generation, variant).unwrap();
    HighWater {
        generation,
        config_hash: report.config_hash,
        route_state_hash: report.route_state_hash,
    }
}

fn tampered(reply: &AttestationReply, from: &str, to: &str) -> Vec<u8> {
    let AttestationReply::Signed { report, .. } = reply else {
        panic!("a signed reply is needed");
    };
    String::from_utf8(report.clone())
        .unwrap()
        .replacen(from, to, 1)
        .into_bytes()
}

#[test]
fn helper_attestation_signature_verifies() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.complete(nonce, 1, 0).unwrap();
    assert!(matches!(rig.run(&reply, None), Verdict::Accepted(_)));
}

#[test]
fn attestation_rejected_for_wrong_key() {
    let rig = rig();
    let nonce = rig.nonce();
    let other = TestHelper::new(2);
    let reply = other.complete(nonce, 1, 0).unwrap();
    match rig.run(&reply, None) {
        Verdict::Revoking {
            outcome,
            reason,
            detail,
        } => {
            assert_eq!(outcome, LatestOutcome::Mismatch);
            assert_eq!(reason, UnverifiedReason::KeyMismatch);
            assert_eq!(detail, "key_mismatch");
        }
        other => panic!("expected a mismatch, got {other:?}"),
    }
    assert!(rig.still_owed(&nonce), "a wrong key never burns the nonce");
}

#[test]
fn attestation_rejected_for_other_host() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.complete(nonce, 1, 0).unwrap();
    let verdict = rig.run_at(&reply, None, t0(), Some(&rig.pin), HostId::new());
    assert_eq!(
        rejected(&verdict),
        (UnverifiedReason::AttestationMissing, "nonce_other_host")
    );
    assert!(rig.still_owed(&nonce), "another host's attempt keeps it");
}

#[test]
fn helper_attestation_rejects_forged_report() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.complete(nonce, 1, 0).unwrap();
    let AttestationReply::Signed {
        public_key,
        signature,
        ..
    } = &reply
    else {
        panic!("signed");
    };
    let forged = AttestationReply::Signed {
        public_key: *public_key,
        signature: *signature,
        report: tampered(&reply, "max_active=0", "max_active=9"),
    };
    assert_eq!(
        rejected(&rig.run(&forged, None)),
        (UnverifiedReason::SignatureInvalid, "signature_invalid")
    );
}

#[test]
fn helper_attestation_rejects_replayed_nonce() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.complete(nonce, 1, 0).unwrap();
    assert!(matches!(rig.run(&reply, None), Verdict::Accepted(_)));
    assert_eq!(
        rejected(&rig.run(&reply, None)),
        (UnverifiedReason::AttestationMissing, "nonce_unknown")
    );
}

#[test]
fn forged_reply_does_not_burn_the_nonce() {
    let rig = rig();
    let nonce = rig.nonce();
    let honest = rig.helper.complete(nonce, 1, 0).unwrap();
    let forged = rig
        .helper
        .sign(tampered(&honest, "generation=1", "generation=2"));
    let AttestationReply::Signed { report, .. } = &forged else {
        panic!("signed");
    };
    let AttestationReply::Signed { public_key, .. } = &honest else {
        panic!("signed");
    };
    let bad_signature = AttestationReply::Signed {
        public_key: *public_key,
        report: report.clone(),
        signature: [7; 64],
    };
    assert_eq!(
        rejected(&rig.run(&bad_signature, None)).0,
        UnverifiedReason::SignatureInvalid
    );
    assert!(matches!(rig.run(&honest, None), Verdict::Accepted(_)));
}

#[test]
fn nonce_is_consumed_only_after_signature_parse_and_hash_checks() {
    let rig = rig();
    let nonce = rig.nonce();
    let honest = rig.helper.complete(nonce, 1, 0).unwrap();
    // Validly signed, but not a report: the parse step refuses it.
    let garbage = rig.helper.sign(b"not a report\n".to_vec());
    assert_eq!(
        rejected(&rig.run(&garbage, None)),
        (UnverifiedReason::ReportIncomplete, "report_malformed")
    );
    // Validly signed and well formed, but a hash does not match its lines.
    let zero = "0".repeat(64);
    let real = {
        let report = complete_report(nonce, 1, 0).unwrap();
        report.config_hash.to_string()
    };
    let bad_hash = rig.helper.sign(tampered(&honest, &real, &"1".repeat(64)));
    assert_ne!(zero, real);
    assert_eq!(
        rejected(&rig.run(&bad_hash, None)),
        (UnverifiedReason::ReportIncomplete, "hash_mismatch")
    );
    // The nonce is still owed to the honest answer.
    assert!(matches!(rig.run(&honest, None), Verdict::Accepted(_)));
}

#[test]
fn nonce_boundaries_are_strict_at_60_and_300_seconds() {
    let nonces = NonceStore::new();
    let host = HostId::new();
    let just_under = NONCE_TTL - SignedDuration::from_millis(1);
    let nonce = nonces.issue(host, t0());
    assert_eq!(nonces.take(host, &nonce, t0() + just_under), Ok(t0()));
    let nonce = nonces.issue(host, t0());
    assert_eq!(
        nonces.take(host, &nonce, t0() + NONCE_TTL),
        Err(NonceError::Expired)
    );
    assert_eq!(
        nonces.take(host, &nonce, t0()),
        Err(NonceError::Unknown),
        "an expired nonce is consumed too"
    );
    // Freshness is strict at 300 seconds, measured from the issue time.
    let helper = TestHelper::new(1);
    let view = verified_view(&helper, 1, 0, t0()).unwrap();
    let almost = FRESHNESS - SignedDuration::from_millis(1);
    assert!(gate(t0() + almost, &view).is_ok());
    assert_eq!(
        gate(t0() + FRESHNESS, &view).unwrap_err(),
        UnverifiedReason::AttestationStale
    );
}

#[test]
fn signed_incomplete_report_revokes_and_consumes_the_nonce() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.incomplete(nonce, "state_corrupt").unwrap();
    match rig.run(&reply, None) {
        Verdict::Revoking {
            outcome,
            reason,
            detail,
        } => {
            assert_eq!(outcome, LatestOutcome::SignedIncomplete);
            assert_eq!(reason, UnverifiedReason::ReportIncomplete);
            assert_eq!(detail, "helper_incomplete:state_corrupt");
        }
        other => panic!("expected a revocation, got {other:?}"),
    }
    assert!(!rig.still_owed(&nonce));
}

#[test]
fn incomplete_reason_empty_only_when_complete() {
    let rig = rig();
    let nonce = rig.nonce();
    let incomplete = rig.helper.incomplete(nonce, "state_missing").unwrap();
    let empty_reason = rig.helper.sign(tampered(
        &incomplete,
        "incomplete_reason=state_missing",
        "incomplete_reason=",
    ));
    assert_eq!(
        rejected(&rig.run(&empty_reason, None)).1,
        "report_malformed"
    );
    let complete = rig.helper.complete(nonce, 1, 0).unwrap();
    let reason_on_complete = rig.helper.sign(tampered(
        &complete,
        "incomplete_reason=\n",
        "incomplete_reason=state_missing\n",
    ));
    assert_eq!(
        rejected(&rig.run(&reason_on_complete, None)).1,
        "report_malformed"
    );
}

#[test]
fn incomplete_report_has_minimal_shape() {
    let rig = rig();
    let nonce = rig.nonce();
    let incomplete = rig.helper.incomplete(nonce, "state_missing").unwrap();
    for (from, to) in [
        ("max_active=0", "max_active=3"),
        ("max_ttl_s=0", "max_ttl_s=60"),
        ("generation=0", "generation=4"),
        ("max_active=0\n", "max_active=0\ncidr=192.0.2.0/24\n"),
    ] {
        let widened = rig.helper.sign(tampered(&incomplete, from, to));
        assert_eq!(rejected(&rig.run(&widened, None)).1, "report_malformed");
    }
}

#[test]
fn generation_rules_follow_the_high_water_mark() {
    let rig = rig();
    let nonce = rig.nonce();
    let lower = rig.helper.complete(nonce, 2, 0).unwrap();
    match rig.run(&lower, Some(&water(3, 0))) {
        Verdict::Revoking {
            outcome, detail, ..
        } => {
            assert_eq!(outcome, LatestOutcome::Regressed);
            assert_eq!(detail, "generation_regressed");
        }
        other => panic!("expected a regression, got {other:?}"),
    }
    let nonce = rig.nonce();
    let repeated = rig.helper.complete(nonce, 3, 1).unwrap();
    match rig.run(&repeated, Some(&water(3, 0))) {
        Verdict::Revoking {
            outcome, detail, ..
        } => {
            assert_eq!(outcome, LatestOutcome::Regressed);
            assert_eq!(detail, "generation_repeated");
        }
        other => panic!("expected a repeat, got {other:?}"),
    }
    let nonce = rig.nonce();
    let same = rig.helper.complete(nonce, 3, 0).unwrap();
    assert!(matches!(
        rig.run(&same, Some(&water(3, 0))),
        Verdict::Accepted(_)
    ));
    let nonce = rig.nonce();
    let higher = rig.helper.complete(nonce, 4, 1).unwrap();
    assert!(matches!(
        rig.run(&higher, Some(&water(3, 0))),
        Verdict::Accepted(_)
    ));
}

#[test]
fn key_id_never_used_for_verification() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.complete(nonce, 1, 0).unwrap();
    let relabelled = rig.helper.sign(tampered(
        &reply,
        "helper_key_id=0123456789abcdef",
        "helper_key_id=ffffffffffffffff",
    ));
    assert!(matches!(rig.run(&relabelled, None), Verdict::Accepted(_)));
    // A report carrying the pinned key's own id, signed by another key, is a mismatch.
    let nonce = rig.nonce();
    let id: String = rig.pin.fingerprint.to_string().chars().take(16).collect();
    let impostor = TestHelper::new(9).sign(render_complete(&{
        let mut report = complete_report(nonce, 1, 0).unwrap();
        report.helper_key_id = id;
        report
    }));
    assert!(matches!(
        rig.run(&impostor, None),
        Verdict::Revoking {
            outcome: LatestOutcome::Mismatch,
            ..
        }
    ));
}

#[test]
fn without_a_pin_nothing_verifies() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.complete(nonce, 1, 0).unwrap();
    let verdict = rig.run_at(&reply, None, t0(), None, rig.host);
    assert_eq!(
        rejected(&verdict),
        (UnverifiedReason::KeyNotPinned, "key_not_pinned")
    );
    assert!(rig.still_owed(&nonce));
}

#[test]
fn computed_at_never_decides_anything() {
    let rig = rig();
    let nonce = rig.nonce();
    let reply = rig.helper.complete(nonce, 1, 0).unwrap();
    let far_future = rig.helper.sign(tampered(
        &reply,
        "computed_at=2023-11-14T22:13:20Z",
        "computed_at=2099-01-01T00:00:00Z",
    ));
    assert!(matches!(rig.run(&far_future, None), Verdict::Accepted(_)));
}

mod schedule {
    use std::sync::Arc;

    use domovoi_core::ports::{
        HelperLive, HelperReport, HelperTrustRepo, HostReconciler, HostRepo, Presence,
    };
    use domovoi_core::{Actor, UserId};
    use domovoi_testkit::build::host;
    use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};

    use super::*;
    use crate::helper_trust::{Attempt, AttestOnEvent, HelperTrustHub};

    struct Rig {
        hub: Arc<HelperTrustHub>,
        agent: Arc<FakeAgentLink>,
        store: Arc<InMemoryStore>,
        helper: Arc<TestHelper>,
    }

    async fn rig(host_name: &str) -> (Rig, HostId) {
        let store = Arc::new(InMemoryStore::new());
        let link = Arc::new(FakeAgentLink::new());
        let clock = Arc::new(FakeClock::new(t0()));
        let registered = host(host_name).unwrap();
        store.upsert_host(&registered).await.unwrap();
        let hub = Arc::new(HelperTrustHub::new(
            store.clone(),
            store.clone(),
            link.clone(),
            clock.clone(),
        ));
        let helper = Arc::new(TestHelper::new(1));
        link.set_presence(
            registered.id,
            Presence::Connected {
                since: t0(),
                version: "0.1.0".to_owned(),
            },
        );
        link.set_helper_report(
            registered.id,
            HelperReport {
                live: HelperLive::Ok {
                    ops: vec!["safety.attest".to_owned()],
                },
                public_key: Some(helper.public_key()),
            },
        );
        let signer = helper.clone();
        link.script_attestation(
            registered.id,
            Arc::new(move |_, nonce| Ok(signer.complete(nonce, 1, 0).unwrap())),
        );
        let actor = Actor::User(UserId::new());
        hub.pin_key(actor, registered.id, helper.public_key())
            .await
            .unwrap();
        (
            Rig {
                hub,
                agent: link,
                store,
                helper,
            },
            registered.id,
        )
    }

    #[tokio::test]
    async fn the_periodic_pass_asks_connected_hosts_whose_helper_is_present() {
        let (rig, id) = rig("nas").await;
        rig.hub.request_all(rig.store.as_ref()).await;
        assert_eq!(rig.agent.attestation_requests().len(), 1);
        let stored = rig.store.stored(id).await.unwrap();
        assert!(stored.view.verified.is_some());
        // Not connected, or no helper reported: not asked.
        rig.agent.set_presence(
            id,
            Presence::Disconnected {
                last_seen: Some(t0()),
            },
        );
        rig.hub.request_all(rig.store.as_ref()).await;
        rig.agent.set_presence(
            id,
            Presence::Connected {
                since: t0(),
                version: "0.1.0".to_owned(),
            },
        );
        rig.agent.set_helper_report(
            id,
            HelperReport {
                live: HelperLive::Absent,
                public_key: None,
            },
        );
        rig.hub.request_all(rig.store.as_ref()).await;
        assert_eq!(rig.agent.attestation_requests().len(), 1);
    }

    #[tokio::test]
    async fn authenticating_or_reporting_status_asks_the_helper() {
        let (rig, id) = rig("nas").await;
        let on_event = AttestOnEvent::new(rig.hub.clone());
        on_event.reconcile_host(id).await;
        assert_eq!(rig.agent.attestation_requests().len(), 1);
        assert!(rig.store.stored(id).await.unwrap().view.verified.is_some());
        let _ = &rig.helper;
    }

    #[tokio::test]
    async fn one_request_per_host_runs_at_a_time() {
        let (rig, id) = rig("nas").await;
        let slot = rig.hub.begin(id).expect("the first request takes the slot");
        assert_eq!(rig.hub.request(id).await.unwrap(), Attempt::Busy);
        assert!(rig.agent.attestation_requests().is_empty());
        drop(slot);
        assert_eq!(rig.hub.request(id).await.unwrap(), Attempt::Verified);
    }

    #[tokio::test]
    async fn a_host_without_a_pin_is_not_asked() {
        let (rig, id) = rig("nas").await;
        rig.hub
            .unpin_key(Actor::User(UserId::new()), id)
            .await
            .unwrap();
        assert_eq!(rig.hub.request(id).await.unwrap(), Attempt::NotPinned);
        assert!(rig.agent.attestation_requests().is_empty());
    }
}
