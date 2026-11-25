//! Helper trust end to end, in-process over the router: the key the hub receives, the owner's
//! pin behind the password and the typed fingerprint, the attestation the hub asks for, what
//! it verifies, and what it records when a report fails.

use std::sync::{Arc, Mutex};

use axum::http::{Method, StatusCode};
use domovoi_core::attest::Nonce32;
use domovoi_core::ports::{
    AttestationReply, HelperLive, HelperReport, HelperTrustRepo, LinkError, Presence,
};
use domovoi_core::{HostId, JournalEpoch};
use domovoi_testkit::helper_attest::TestHelper;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api_harness::{Api, Login, PASSWORD, Reply, api, t0};

/// How the scripted helper answers the next request.
#[derive(Clone)]
enum Mode {
    /// A complete report at `generation` of configuration `variant`.
    Honest { generation: u64, variant: u64 },
    /// A signed incomplete report.
    Incomplete(&'static str),
    /// An unsigned token relayed as an error.
    Unsigned(&'static str),
    /// No answer in time.
    Timeout,
    /// The honest report with one signature byte flipped.
    BadSignature,
}

struct Rig {
    a: Api,
    login: Login,
    helper: Arc<Mutex<Arc<TestHelper>>>,
    mode: Arc<Mutex<Mode>>,
    host: HostId,
}

fn connect(a: &Api, host: HostId) {
    a.link.set_presence(
        host,
        Presence::Connected {
            since: t0(),
            version: "0.1.0".to_owned(),
        },
    );
    a.link.set_journal_epoch(
        host,
        JournalEpoch::from_uuid(Uuid::from_u128(host.as_uuid().as_u128())),
    );
}

fn announce(a: &Api, host: HostId, helper: &TestHelper) {
    a.link.set_helper_report(
        host,
        HelperReport {
            live: HelperLive::Ok {
                ops: vec!["safety.attest".to_owned()],
            },
            public_key: Some(helper.public_key()),
        },
    );
}

async fn rig() -> Rig {
    let a = api().await;
    let login = a.admin().await;
    let host = a.nas.id;
    connect(&a, host);
    let helper = Arc::new(Mutex::new(Arc::new(TestHelper::new(1))));
    let mode = Arc::new(Mutex::new(Mode::Honest {
        generation: 1,
        variant: 0,
    }));
    announce(&a, host, &helper.lock().unwrap());
    let (script_helper, script_mode) = (helper.clone(), mode.clone());
    a.link.script_attestation(
        host,
        Arc::new(move |_, nonce: Nonce32| {
            let helper = script_helper.lock().unwrap().clone();
            let mode = script_mode.lock().unwrap().clone();
            match mode {
                Mode::Honest {
                    generation,
                    variant,
                } => Ok(helper.complete(nonce, generation, variant).unwrap()),
                Mode::Incomplete(reason) => Ok(helper.incomplete(nonce, reason).unwrap()),
                Mode::Unsigned(token) => Ok(AttestationReply::Unsigned {
                    token: token.to_owned(),
                }),
                Mode::Timeout => Err(LinkError::Timeout),
                Mode::BadSignature => match helper.complete(nonce, 1, 0).unwrap() {
                    AttestationReply::Signed {
                        public_key,
                        report,
                        mut signature,
                    } => {
                        signature[0] ^= 1;
                        Ok(AttestationReply::Signed {
                            public_key,
                            report,
                            signature,
                        })
                    }
                    unsigned => Ok(unsigned),
                },
            }
        }),
    );
    Rig {
        a,
        login,
        helper,
        mode,
        host,
    }
}

impl Rig {
    fn helper(&self) -> Arc<TestHelper> {
        self.helper.lock().unwrap().clone()
    }

    fn say(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    /// The helper changes its key (or an agent substitutes one) and the session relays it.
    fn swap_key(&self, seed: u8) {
        let helper = Arc::new(TestHelper::new(seed));
        announce(&self.a, self.host, &helper);
        *self.helper.lock().unwrap() = helper;
    }

    async fn view(&self) -> Value {
        let reply = self.a.get("/api/v1/hosts/nas/helper", &self.login).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        reply.body
    }

    async fn recheck(&self) -> Reply {
        self.a.advance(jiff::SignedDuration::from_secs(11));
        self.a
            .post("/api/v1/hosts/nas/helper/recheck", &self.login, json!({}))
            .await
    }

    async fn asked(&self) -> Value {
        let reply = self.recheck().await;
        assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.text);
        self.view().await
    }

    async fn pin_with(&self, fingerprint: &str, password: Option<&str>) -> Reply {
        let body = match password {
            Some(password) => json!({"fingerprint": fingerprint, "password": password}),
            None => json!({"fingerprint": fingerprint}),
        };
        self.a
            .send(
                Method::PUT,
                "/api/v1/hosts/nas/helper/key",
                Some(&self.login),
                Some(body),
            )
            .await
    }

    async fn pin(&self) -> Value {
        let fingerprint = self.helper().fingerprint().to_string();
        let reply = self.pin_with(&fingerprint, Some(PASSWORD)).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        reply.body
    }

    async fn unpin(&self) -> Reply {
        self.a
            .send(
                Method::DELETE,
                "/api/v1/hosts/nas/helper/key",
                Some(&self.login),
                Some(json!({"password": PASSWORD})),
            )
            .await
    }

    /// Pins the current key and verifies one attestation.
    async fn verified(&self) -> Value {
        self.pin().await;
        let view = self.asked().await;
        assert_eq!(view["attestation"]["state"], "verified", "{view}");
        view
    }
}

#[tokio::test]
async fn helper_public_key_exchange_and_fingerprint() {
    let rig = rig().await;
    let view = rig.view().await;
    let fingerprint = rig.helper().fingerprint().to_string();
    assert_eq!(view["host"], "nas");
    assert_eq!(view["helper"]["state"], "ok");
    assert_eq!(view["received_key"]["fingerprint"], fingerprint);
    assert_eq!(view["received_key"]["key_id"], fingerprint[..16]);
    let groups = view["received_key"]["fingerprint_groups"].as_str().unwrap();
    assert_eq!(groups.replace(' ', ""), fingerprint);
    assert!(groups.split(' ').all(|group| group.len() == 4));
    assert_eq!(view["key_state"], "not_pinned");
    assert_eq!(view["pin"], Value::Null);
}

#[tokio::test]
async fn hub_ignores_any_agent_supplied_fingerprint() {
    // The agent can only relay key bytes; the fingerprint shown is the hub's own hash of them.
    let rig = rig().await;
    let view = rig.view().await;
    let expected = domovoi_core::KeyFingerprint::of_public_key(&rig.helper().public_key());
    assert_eq!(view["received_key"]["fingerprint"], expected.to_string());
}

#[tokio::test]
async fn first_contact_does_not_pin_key() {
    let rig = rig().await;
    let view = rig.asked().await;
    assert_eq!(view["pin"], Value::Null);
    assert_eq!(view["attestation"]["state"], "unverified");
    assert_eq!(view["attestation"]["reason"], "key_not_pinned");
    assert_eq!(view["verified"], Value::Null);
    assert!(
        rig.a.link.attestation_requests().is_empty(),
        "a host without a pin is not asked"
    );
}

#[tokio::test]
async fn pin_requires_password_and_full_typed_fingerprint() {
    let rig = rig().await;
    let fingerprint = rig.helper().fingerprint().to_string();
    let missing = rig.pin_with(&fingerprint, None).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    let wrong = rig.pin_with(&fingerprint, Some("not the password")).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.body["error"], "wrong_password");
    for typed in [&fingerprint[..63], &fingerprint[..32], "", &"0".repeat(64)] {
        let refused = rig.pin_with(typed, Some(PASSWORD)).await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY, "{typed}");
        assert_eq!(refused.body["error"], "fingerprint_mismatch");
    }
    assert_eq!(rig.view().await["pin"], Value::Null, "nothing was pinned");
    // Spaces and case are ignored.
    let spaced = fingerprint
        .as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).to_uppercase())
        .collect::<Vec<_>>()
        .join(" ");
    let pinned = rig.pin_with(&spaced, Some(PASSWORD)).await;
    assert_eq!(pinned.status, StatusCode::OK, "{}", pinned.text);
    assert_eq!(pinned.body["pin"]["fingerprint"], fingerprint);
    assert_eq!(pinned.body["key_state"], "pinned");
}

#[tokio::test]
async fn pin_with_key_id_only_is_refused() {
    let rig = rig().await;
    let id = rig.helper().fingerprint().to_string()[..16].to_owned();
    let refused = rig.pin_with(&id, Some(PASSWORD)).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(rig.view().await["pin"], Value::Null);
}

#[tokio::test]
async fn pin_without_a_held_key_is_a_conflict() {
    let rig = rig().await;
    rig.a.link.set_helper_report(
        rig.host,
        HelperReport {
            live: HelperLive::Absent,
            public_key: None,
        },
    );
    let reply = rig.pin_with(&"a".repeat(64), Some(PASSWORD)).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.body["error"], "no_helper_key");
    assert_eq!(rig.view().await["key_state"], "no_helper");
}

#[tokio::test]
async fn pin_audits_key_pinned_then_key_rotated_with_old_and_new_fingerprints() {
    let rig = rig().await;
    let first = rig.helper().fingerprint().to_string();
    rig.pin().await;
    let pinned = rig.a.audit_events("helper.key_pinned").await;
    assert_eq!(pinned.len(), 1);
    assert_eq!(pinned[0].detail["fingerprint"], first);
    rig.swap_key(2);
    let second = rig.helper().fingerprint().to_string();
    rig.pin().await;
    let rotated = rig.a.audit_events("helper.key_rotated").await;
    assert_eq!(rotated.len(), 1);
    assert_eq!(rotated[0].detail["old_fingerprint"], first);
    assert_eq!(rotated[0].detail["new_fingerprint"], second);
}

#[tokio::test]
async fn key_substitution_by_agent_is_detected() {
    let rig = rig().await;
    rig.verified().await;
    rig.swap_key(2);
    let view = rig.view().await;
    assert_eq!(view["key_state"], "changed");
    assert_eq!(view["attestation"]["reason"], "key_mismatch");
    assert_eq!(
        view["verified"],
        Value::Null,
        "a changed key blocks at once"
    );
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["reason"], "key_mismatch");
    assert_eq!(rig.a.audit_events("helper.key_mismatch").await.len(), 1);
    assert_eq!(
        rig.a.audit_events("helper.attestation_revoked").await.len(),
        1
    );
}

#[tokio::test]
async fn key_rotation_blocks_until_reenrolled() {
    let rig = rig().await;
    rig.verified().await;
    rig.swap_key(3);
    assert_eq!(rig.asked().await["attestation"]["state"], "unverified");
    rig.pin().await;
    let view = rig.asked().await;
    assert_eq!(view["key_state"], "pinned");
    assert_eq!(view["attestation"]["state"], "verified");
}

#[tokio::test]
async fn unpin_blocks_containment() {
    let rig = rig().await;
    rig.verified().await;
    let reply = rig.unpin().await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["pin"], Value::Null);
    assert_eq!(reply.body["attestation"]["reason"], "key_not_pinned");
    assert_eq!(reply.body["verified"], Value::Null);
    assert_eq!(rig.a.audit_events("helper.key_unpinned").await.len(), 1);
}

#[tokio::test]
async fn agent_reenrollment_keeps_helper_pin() {
    let rig = rig().await;
    rig.pin().await;
    rig.a.link.set_presence(
        rig.host,
        Presence::Disconnected {
            last_seen: Some(t0()),
        },
    );
    connect(&rig.a, rig.host);
    let view = rig.view().await;
    assert_eq!(view["key_state"], "pinned");
    assert_eq!(
        view["pin"]["fingerprint"],
        rig.helper().fingerprint().to_string()
    );
}

#[tokio::test]
async fn owner_repin_resets_high_water_for_that_key() {
    let rig = rig().await;
    rig.pin().await;
    rig.say(Mode::Honest {
        generation: 5,
        variant: 0,
    });
    assert_eq!(rig.asked().await["attestation"]["state"], "verified");
    rig.say(Mode::Honest {
        generation: 3,
        variant: 0,
    });
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["reason"], "generation_regressed");
    rig.pin().await;
    assert_eq!(rig.a.audit_events("helper.high_water_reset").await.len(), 1);
    assert_eq!(rig.asked().await["attestation"]["state"], "verified");
}

#[tokio::test]
async fn rotation_starts_a_new_high_water_row() {
    let rig = rig().await;
    rig.pin().await;
    rig.say(Mode::Honest {
        generation: 5,
        variant: 0,
    });
    rig.asked().await;
    let first = rig.helper().fingerprint();
    rig.swap_key(4);
    rig.pin().await;
    rig.say(Mode::Honest {
        generation: 1,
        variant: 0,
    });
    assert_eq!(rig.asked().await["attestation"]["state"], "verified");
    let old = rig.a.store.high_water(rig.host, &first).await.unwrap();
    assert_eq!(old.map(|row| row.generation), Some(5), "the old mark stays");
    let new = rig
        .a
        .store
        .high_water(rig.host, &rig.helper().fingerprint())
        .await
        .unwrap();
    assert_eq!(new.map(|row| row.generation), Some(1));
}

#[tokio::test]
async fn same_key_repin_after_mismatch_unblocks() {
    let rig = rig().await;
    rig.verified().await;
    let original = rig.helper();
    rig.swap_key(2);
    assert_eq!(rig.asked().await["attestation"]["reason"], "key_mismatch");
    *rig.helper.lock().unwrap() = original.clone();
    announce(&rig.a, rig.host, &original);
    rig.pin().await;
    assert_eq!(rig.asked().await["attestation"]["state"], "verified");
}

#[tokio::test]
async fn hub_rejects_generation_regression() {
    let rig = rig().await;
    rig.pin().await;
    rig.say(Mode::Honest {
        generation: 5,
        variant: 0,
    });
    rig.asked().await;
    rig.say(Mode::Honest {
        generation: 4,
        variant: 0,
    });
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["reason"], "generation_regressed");
    assert_eq!(view["verified"], Value::Null);
    assert_eq!(
        rig.a
            .audit_events("helper.generation_regressed")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn repeated_generation_with_different_hashes_rejected() {
    let rig = rig().await;
    rig.pin().await;
    rig.say(Mode::Honest {
        generation: 5,
        variant: 0,
    });
    rig.asked().await;
    rig.say(Mode::Honest {
        generation: 5,
        variant: 1,
    });
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["reason"], "generation_regressed");
    rig.say(Mode::Honest {
        generation: 6,
        variant: 1,
    });
    assert_eq!(rig.asked().await["attestation"]["state"], "verified");
}

#[tokio::test]
async fn signed_incomplete_revokes_verified_state() {
    let rig = rig().await;
    rig.verified().await;
    rig.say(Mode::Incomplete("state_corrupt"));
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["state"], "unverified");
    assert_eq!(view["attestation"]["reason"], "report_incomplete");
    assert_eq!(view["attestation"]["helper_reason"], "state_corrupt");
    assert_eq!(view["verified"], Value::Null);
    assert_eq!(
        rig.a.audit_events("helper.attestation_revoked").await.len(),
        1
    );
}

#[tokio::test]
async fn mismatch_cleared_by_next_verified_attestation() {
    let rig = rig().await;
    rig.verified().await;
    // A wrong key answers once, then the honest helper answers again.
    let honest = rig.helper();
    rig.swap_key(2);
    rig.asked().await;
    *rig.helper.lock().unwrap() = honest.clone();
    announce(&rig.a, rig.host, &honest);
    rig.say(Mode::Honest {
        generation: 2,
        variant: 0,
    });
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["state"], "verified", "{view}");
    assert_eq!(
        rig.a
            .audit_events("helper.attestation_restored")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn freshness_measured_from_nonce_issue_time() {
    let rig = rig().await;
    rig.verified().await;
    rig.a.advance(jiff::SignedDuration::from_secs(299));
    let fresh = rig.view().await;
    assert_eq!(fresh["attestation"]["state"], "verified");
    assert_eq!(fresh["verified"]["age_seconds"], 299);
    assert_eq!(fresh["verified"]["fresh"], true);
    rig.a.advance(jiff::SignedDuration::from_secs(1));
    let stale = rig.view().await;
    assert_eq!(stale["attestation"]["state"], "stale");
    assert_eq!(stale["attestation"]["reason"], "attestation_stale");
    assert_eq!(stale["verified"], Value::Null);
}

#[tokio::test]
async fn unsigned_token_gates_attestation_missing() {
    let rig = rig().await;
    rig.pin().await;
    rig.say(Mode::Unsigned("key_missing"));
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["state"], "unverified");
    assert_eq!(view["attestation"]["reason"], "attestation_missing");
    assert_eq!(view["attestation"]["unsigned_hint"], "key_missing");
    assert_eq!(view["verified"], Value::Null);
}

#[tokio::test]
async fn error_frame_reply_does_not_wait_for_timeout() {
    let rig = rig().await;
    rig.pin().await;
    rig.say(Mode::Unsigned("helper_unavailable"));
    let before = rig.a.now();
    let reply = rig.recheck().await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    // The fake clock moved only by the pre-request gap: the hub did not sleep to a deadline.
    assert_eq!(
        rig.a.now().duration_since(before).as_secs(),
        11,
        "the answer was used at once"
    );
    assert_eq!(rig.a.link.attestation_requests().len(), 1);
}

#[tokio::test]
async fn link_timeout_revokes_nothing() {
    let rig = rig().await;
    rig.verified().await;
    rig.say(Mode::Timeout);
    let view = rig.asked().await;
    assert_eq!(view["attestation"]["state"], "verified");
    assert!(view["verified"].is_object());
}

#[tokio::test]
async fn hub_rejects_unverified_safety_report() {
    let rig = rig().await;
    rig.verified().await;
    rig.say(Mode::BadSignature);
    let view = rig.asked().await;
    // The forged reply changes nothing: the earlier verified state stands until it ages out.
    assert_eq!(view["attestation"]["state"], "verified");
    assert_eq!(view["verified"]["generation"], 1);
    rig.a.advance(jiff::SignedDuration::from_secs(300));
    let view = rig.view().await;
    assert_eq!(view["attestation"]["state"], "stale");
}

#[tokio::test]
async fn recheck_is_rate_limited() {
    let rig = rig().await;
    rig.pin().await;
    let first = rig.recheck().await;
    assert_eq!(first.status, StatusCode::ACCEPTED);
    let second = rig
        .a
        .post("/api/v1/hosts/nas/helper/recheck", &rig.login, json!({}))
        .await;
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(second.body["error"], "recheck_too_soon");
    assert_eq!(rig.a.link.attestation_requests().len(), 1);
}

#[tokio::test]
async fn recheck_on_an_offline_host_is_refused() {
    let rig = rig().await;
    rig.a.link.set_presence(
        rig.host,
        Presence::Disconnected {
            last_seen: Some(t0()),
        },
    );
    let reply = rig.recheck().await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert!(rig.a.link.attestation_requests().is_empty());
}

#[tokio::test]
async fn helper_dto_is_hub_computed_and_omits_verified_when_the_gate_fails() {
    let rig = rig().await;
    let before = rig.view().await;
    assert_eq!(before["verified"], Value::Null);
    let view = rig.verified().await;
    let verified = &view["verified"];
    assert_eq!(verified["generation"], 1);
    assert_eq!(verified["max_ttl_s"], 7200);
    assert_eq!(verified["cidrs"], json!(["192.0.2.0/24", "2001:db8::/32"]));
    assert_eq!(verified["hub_addrs"], json!(["192.0.2.10"]));
    assert!(verified["protected_units"].as_array().unwrap().len() >= 4);
    assert_eq!(verified["protected_containers"], json!(["ctr-a"]));
    assert_eq!(verified["boot_id"], domovoi_testkit::helper_attest::BOOT);
    rig.say(Mode::Incomplete("route_unrepresentable"));
    let failed = rig.asked().await;
    assert_eq!(failed["verified"], Value::Null);
    assert_eq!(
        failed["attestation"]["helper_reason"],
        "route_unrepresentable"
    );
}

#[tokio::test]
async fn no_audit_row_carries_report_signature_or_nonce() {
    let rig = rig().await;
    rig.verified().await;
    rig.say(Mode::Incomplete("state_missing"));
    rig.asked().await;
    rig.say(Mode::Honest {
        generation: 1,
        variant: 0,
    });
    rig.asked().await;
    rig.swap_key(2);
    rig.asked().await;
    let mut text = String::new();
    for kind in [
        "helper.key_pinned",
        "helper.key_mismatch",
        "helper.attestation_revoked",
        "helper.attestation_restored",
    ] {
        for event in rig.a.audit_events(kind).await {
            text.push_str(&event.detail.to_string());
            text.push_str(&event.subject);
        }
    }
    assert!(!text.is_empty());
    assert!(!text.contains("format=1") && !text.contains("domovoi-safety"));
    for (_, nonce) in rig.a.link.attestation_requests() {
        assert!(
            !text.contains(&nonce.to_string()),
            "a nonce is in an audit row"
        );
    }
}

#[tokio::test]
async fn monitoring_unaffected_by_missing_management_paths() {
    use domovoi_core::UserId;
    use domovoi_core::knowledge::{FactSource, Predicate, SubjectRef, UnverifiableReason};
    use domovoi_engine::knowledge::{
        AttestedCoverage, FactDraft, KnowledgeDeps, KnowledgeError, KnowledgeService,
    };
    use domovoi_hub::helper_trust::HelperTrustHub;

    // The real attested coverage over a host whose helper is never verified.
    let rig = rig().await;
    let hub = Arc::new(HelperTrustHub::new(
        rig.a.store.clone(),
        rig.a.store.clone(),
        rig.a.link.clone(),
        rig.a.clock.clone(),
    ));
    let knowledge = KnowledgeService::new(KnowledgeDeps {
        uow: rig.a.store.clone(),
        read: rig.a.store.clone(),
        coverage: Arc::new(AttestedCoverage::new(hub, rig.a.clock.clone())),
        clock: rig.a.clock.clone(),
    });
    let path = knowledge
        .assert_fact(
            FactDraft {
                subject: SubjectRef::Host(rig.host),
                predicate: Predicate::ManagementPath,
                value: json!({"cidr": "192.0.2.0/24"}),
                source: FactSource::User,
                observed_at: None,
                valid_until: None,
                evidence: None,
            },
            UserId::new(),
        )
        .await
        .unwrap();
    let refused = knowledge
        .confirm_management_path(path.fact.value.id, path.fact.version, UserId::new())
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        KnowledgeError::CoverageUnverifiable(UnverifiableReason::KeyNotPinned)
    );
    // Everything else carries on: the incident is listed, a person acknowledges it, and the
    // hub reports the unverified helper without error.
    let incidents = rig.a.get("/api/v1/incidents", &rig.login).await;
    assert_eq!(incidents.status, StatusCode::OK, "{}", incidents.text);
    assert!(!incidents.body["items"].as_array().unwrap().is_empty());
    let ack = rig
        .a
        .post(
            &format!("/api/v1/incidents/{}/ack", rig.a.incident.id()),
            &rig.login,
            json!({}),
        )
        .await;
    assert_eq!(ack.status, StatusCode::NO_CONTENT, "{}", ack.text);
    let view = rig.view().await;
    assert_eq!(view["attestation"]["reason"], "key_not_pinned");
}
