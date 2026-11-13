//! Certificates, enrollment tokens and the handshake.

use domovoi_core::ports::{
    AgentLink, AuditEvent, AuditFilter, AuditLog, Clock, HostRepo, LinkError, Page,
};
use domovoi_core::{
    AgentId, AuthenticatedReport, HostId, HostKind, HostName, ReadToolCall, ReportedProtocol,
};
use domovoi_hub::agent_server::{
    AgentServer, AgentServerDeps, ProtocolState, TlsIdentity, token_hash,
};
use domovoi_proto::messages::{Enrolled, RejectReason};
use domovoi_proto::{Message, PROTOCOL_VERSION, ProtocolVersion};
use jiff::SignedDuration;

use crate::agent::{FakeAgent, Ident, enroll, hello_running, new_key, rejected};
use crate::harness::{Hub, pump, t0};

#[tokio::test]
async fn certificate_is_generated_once_and_reused() {
    let hub = Hub::start().await;
    let dir = hub.dir.path().join("tls");
    assert!(dir.join("cert.der").exists() && dir.join("key.der").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("key.der"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the key is private");
    }
    let again = TlsIdentity::load_or_create(&dir, "testhub", t0()).unwrap();
    assert_eq!(again.fingerprint(), hub.server.fingerprint());

    // A second server on the same data directory presents the same certificate.
    let deps = AgentServerDeps {
        clock: hub.clock.clone(),
        hosts: hub.store.clone(),
        facts: hub.store.clone(),
        sink: hub.sink.clone(),
        audit: hub.store.clone(),
        probes: hub.probes.clone(),
        hub_host: hub.hub_host,
    };
    let second = AgentServer::new(crate::harness::settings(&hub.dir), deps).unwrap();
    assert_eq!(second.fingerprint(), hub.server.fingerprint());

    let agent = FakeAgent::connect(hub.addr).await;
    assert_eq!(
        agent.cert,
        hub.server.fingerprint(),
        "agents see the pinned certificate"
    );
    assert_eq!(hub.server.fingerprint_text().len(), 32 * 3 - 1);
}

#[tokio::test]
async fn token_for_unknown_host_creates_remote_host_and_command_line() {
    let hub = Hub::start().await;
    let name = HostName::parse("media").unwrap();
    let invite = hub.server.create_token(&name).await.unwrap();
    let host = hub.store.get_host_by_name(&name).await.unwrap().unwrap();
    assert_eq!(host.id, invite.host);
    assert_eq!(host.kind, HostKind::Remote);
    assert_eq!(invite.expires_at, t0() + SignedDuration::from_mins(15));
    assert_eq!(
        invite.token.expose().len(),
        43,
        "32 bytes in unpadded base64url"
    );
    assert_eq!(
        invite.command,
        format!(
            "domovoi agent enroll --hub wss://testhub:7421/ --token {} --fingerprint {} --name media",
            invite.token.expose(),
            hub.server.fingerprint_text()
        )
    );
    assert!(!format!("{invite:?}").contains(invite.token.expose()));
    // A second token for the same host reuses it.
    let again = hub.server.create_token(&name).await.unwrap();
    assert_eq!(again.host, host.id);
    assert_ne!(
        token_hash(again.token.expose()),
        token_hash(invite.token.expose())
    );
}

#[tokio::test]
async fn enrollment_succeeds_once_and_token_cannot_be_reused() {
    let hub = Hub::start().await;
    let (host, token) = hub.token("nas").await;
    let key = new_key();
    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Enrolled(Enrolled {
        agent_id,
        host_id,
        hub_cert_sha256,
    }) = agent.enroll(&token, &key).await
    else {
        panic!("enrollment failed");
    };
    assert_eq!(host_id, host);
    assert_eq!(hub_cert_sha256.0, hub.server.fingerprint());
    let stored = hub.store.get_agent(agent_id).await.unwrap().unwrap();
    assert_eq!(stored.public_key, key.verifying_key().to_bytes());
    assert_eq!(stored.host, host);
    agent.closed().await;

    let mut again = FakeAgent::connect(hub.addr).await;
    let reply = pump(
        &hub.clock,
        SignedDuration::from_millis(100),
        again.enroll(&token, &new_key()),
    )
    .await;
    assert_eq!(rejected(&reply), Some(RejectReason::BadToken));
    again.closed().await;
}

#[tokio::test]
async fn expired_token_is_rejected() {
    let hub = Hub::start().await;
    let invite = hub
        .server
        .create_token_valid_for(
            &HostName::parse("nas").unwrap(),
            SignedDuration::from_mins(1),
        )
        .await
        .unwrap();
    hub.clock.advance(SignedDuration::from_mins(2));
    let mut agent = FakeAgent::connect(hub.addr).await;
    let started = hub.clock.now();
    let reply = pump(
        &hub.clock,
        SignedDuration::from_millis(100),
        agent.enroll(invite.token.expose(), &new_key()),
    )
    .await;
    assert_eq!(rejected(&reply), Some(RejectReason::BadToken));
    assert!(
        hub.clock.now().duration_since(started) >= SignedDuration::from_secs(1),
        "a wrong token waits a second before the answer"
    );
}

#[tokio::test]
async fn unknown_and_revoked_agents_are_rejected() {
    let hub = Hub::start().await;
    let mut stranger = FakeAgent::connect(hub.addr).await;
    let reply = stranger.hello(AgentId::new(), PROTOCOL_VERSION).await;
    assert_eq!(rejected(&reply), Some(RejectReason::UnknownAgent));

    let ident = enroll(&hub, "nas").await;
    assert!(hub.server.revoke_agent(ident.host).await.unwrap());
    let mut revoked = FakeAgent::connect(hub.addr).await;
    let reply = revoked.hello(ident.agent, PROTOCOL_VERSION).await;
    assert_eq!(rejected(&reply), Some(RejectReason::Revoked));
    assert!(
        !hub.server.revoke_agent(ident.host).await.unwrap(),
        "nothing left to revoke"
    );
}

#[tokio::test]
async fn bad_signature_is_rejected() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = agent.hello(ident.agent, PROTOCOL_VERSION).await else {
        panic!("expected a challenge");
    };
    let reply = agent.answer(&challenge, ident.agent, &new_key()).await;
    assert_eq!(rejected(&reply), Some(RejectReason::BadSignature));
    agent.closed().await;

    // A signature over another connection's binding fails too.
    let mut other = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = other.hello(ident.agent, PROTOCOL_VERSION).await else {
        panic!("expected a challenge");
    };
    other.binding = "00".repeat(32);
    let reply = other.answer(&challenge, ident.agent, &ident.key).await;
    assert_eq!(rejected(&reply), Some(RejectReason::BadSignature));
}

/// Rejects `protocol` for `ident` on a fresh connection, after answering the challenge
/// with the enrolled key.
async fn reject_protocol(
    hub: &Hub,
    ident: &Ident,
    protocol: ProtocolVersion,
) -> (Message, jiff::Timestamp) {
    let at = hub.clock.now();
    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = agent.hello(ident.agent, protocol).await else {
        panic!("the hub asks every caller to prove its key first");
    };
    let reply = agent.answer(&challenge, ident.agent, &ident.key).await;
    agent.closed().await;
    (reply, at)
}

async fn protocol_audit(hub: &Hub) -> Vec<AuditEvent> {
    let filter = AuditFilter {
        kind: Some("agent.protocol_reported".to_owned()),
        ..AuditFilter::default()
    };
    AuditLog::list(&*hub.store, &filter, &Page::first(50))
        .await
        .unwrap()
        .items
}

#[tokio::test]
async fn older_minor_agent_rejected_with_reason() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let (reply, at) = reject_protocol(&hub, &ident, ProtocolVersion { major: 1, minor: 0 }).await;
    assert_eq!(
        rejected(&reply),
        Some(RejectReason::Incompatible {
            hub: PROTOCOL_VERSION
        })
    );
    let report = hub
        .store
        .agent_report(ident.agent)
        .await
        .unwrap()
        .expect("the authenticated report is stored");
    assert_eq!(report.protocol, ReportedProtocol { major: 1, minor: 0 });
    assert_eq!(report.incompatible_since, Some(at));
}

#[tokio::test]
async fn newer_minor_agent_rejected_with_reason() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let (reply, at) = reject_protocol(&hub, &ident, ProtocolVersion { major: 1, minor: 2 }).await;
    assert_eq!(
        rejected(&reply),
        Some(RejectReason::Incompatible {
            hub: PROTOCOL_VERSION
        })
    );
    assert_eq!(
        hub.store
            .agent_report(ident.agent)
            .await
            .unwrap()
            .expect("the report is stored")
            .incompatible_since,
        Some(at)
    );
}

#[tokio::test]
async fn same_protocol_different_patch_releases_communicate() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    for release in ["0.0.1", "0.9.9", "dev"] {
        let mut agent = FakeAgent::connect(hub.addr).await;
        agent
            .send(hello_running(ident.agent, PROTOCOL_VERSION, release), None)
            .await;
        let Message::Challenge(challenge) = agent.expect().await.message else {
            panic!("expected a challenge for {release}");
        };
        let reply = agent.answer(&challenge, ident.agent, &ident.key).await;
        assert!(
            matches!(reply, Message::Welcome(_)),
            "release {release} is welcome on the same protocol: {reply:?}"
        );
    }
}

#[tokio::test]
async fn release_floor_no_longer_rejects() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    agent
        .send(hello_running(ident.agent, PROTOCOL_VERSION, "0.0.1"), None)
        .await;
    let Message::Challenge(challenge) = agent.expect().await.message else {
        panic!("the hub asks every caller to prove its key first");
    };
    assert!(
        matches!(
            agent.answer(&challenge, ident.agent, &ident.key).await,
            Message::Welcome(_)
        ),
        "a release number is not a compatibility dimension"
    );
}

/// Nothing about a hello claiming another minor is stored or published before the key is
/// proved. A client that cannot sign leaves no report and no audit entry.
#[tokio::test]
async fn unauthenticated_hello_does_not_persist_incompatibility() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    agent
        .send(
            hello_running(ident.agent, ProtocolVersion { major: 1, minor: 0 }, "0.1.0"),
            None,
        )
        .await;
    let Message::Challenge(challenge) = agent.expect().await.message else {
        panic!("the hub asks every caller to prove its key first");
    };
    let reply = agent.answer_without_the_key(&challenge, ident.agent).await;
    assert_eq!(rejected(&reply), Some(RejectReason::BadSignature));
    assert_eq!(
        hub.store.agent_report(ident.agent).await.unwrap(),
        None,
        "an unauthenticated client must not record a report"
    );
    assert!(protocol_audit(&hub).await.is_empty(), "nor an audit entry");
    assert_eq!(
        hub.server.protocol_state(ident.host).await.unwrap(),
        ProtocolState::Unknown
    );
}

/// The same holds for a hello claiming another major inside a decodable frame: the claim
/// is judged only after the signature verifies, like any other protocol difference.
#[tokio::test]
async fn unauthenticated_major_claim_does_not_persist_incompatibility() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    agent
        .send(
            hello_running(
                ident.agent,
                ProtocolVersion {
                    major: PROTOCOL_VERSION.major + 1,
                    minor: 0,
                },
                "0.1.0",
            ),
            None,
        )
        .await;
    let Message::Challenge(challenge) = agent.expect().await.message else {
        panic!("the hub asks every caller to prove its key first");
    };
    let reply = agent.answer_without_the_key(&challenge, ident.agent).await;
    assert_eq!(rejected(&reply), Some(RejectReason::BadSignature));
    assert_eq!(hub.store.agent_report(ident.agent).await.unwrap(), None);
    assert!(protocol_audit(&hub).await.is_empty());
    assert_eq!(
        hub.server.protocol_state(ident.host).await.unwrap(),
        ProtocolState::Unknown
    );
}

/// A hello claiming another major inside a decodable frame is authenticated first and then
/// judged by the same exact rule as a minor difference.
#[tokio::test]
async fn incompatible_major_claim_is_rejected_after_authentication() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let newer = ProtocolVersion {
        major: PROTOCOL_VERSION.major + 1,
        minor: 0,
    };
    let (reply, at) = reject_protocol(&hub, &ident, newer).await;
    assert_eq!(
        rejected(&reply),
        Some(RejectReason::Incompatible {
            hub: PROTOCOL_VERSION
        }),
        "a decodable frame carrying another major is judged by the exact rule"
    );
    let report = hub
        .store
        .agent_report(ident.agent)
        .await
        .unwrap()
        .expect("the report is stored");
    assert_eq!(
        report.protocol,
        ReportedProtocol {
            major: newer.major,
            minor: 0
        }
    );
    assert_eq!(report.incompatible_since, Some(at));
}

#[tokio::test]
async fn unknown_agent_with_old_protocol_gets_unknown_agent_reject() {
    let hub = Hub::start().await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    let unknown = AgentId::new();
    let reply = agent
        .hello(unknown, ProtocolVersion { major: 1, minor: 0 })
        .await;
    assert_eq!(rejected(&reply), Some(RejectReason::UnknownAgent));
    assert_eq!(hub.store.agent_report(unknown).await.unwrap(), None);
}

#[tokio::test]
async fn incompatible_state_survives_hub_restart() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let (reply, at) = reject_protocol(&hub, &ident, ProtocolVersion { major: 1, minor: 0 }).await;
    assert!(matches!(reply, Message::Reject(_)));
    let restarted = hub.restart();
    assert_eq!(
        restarted.protocol_state(ident.host).await.unwrap(),
        ProtocolState::Incompatible {
            reported: ProtocolVersion { major: 1, minor: 0 },
            since: at,
        }
    );
}

#[tokio::test]
async fn repeated_mismatch_writes_once() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let old = ProtocolVersion { major: 1, minor: 0 };
    for _ in 0..5 {
        let (reply, _) = reject_protocol(&hub, &ident, old).await;
        assert!(matches!(reply, Message::Reject(_)));
    }
    let at = hub.clock.now();
    assert_eq!(
        hub.store
            .agent_report(ident.agent)
            .await
            .unwrap()
            .map(|report| report.last_authenticated_at),
        Some(at)
    );
    assert_eq!(
        protocol_audit(&hub).await.len(),
        1,
        "one change, one audit entry"
    );

    hub.clock.advance(SignedDuration::from_mins(61));
    let (reply, _) = reject_protocol(&hub, &ident, old).await;
    assert!(matches!(reply, Message::Reject(_)));
    assert_eq!(
        hub.store
            .agent_report(ident.agent)
            .await
            .unwrap()
            .map(|report| report.last_authenticated_at),
        Some(hub.clock.now()),
        "the hourly refresh advances the timestamp"
    );
    assert_eq!(
        protocol_audit(&hub).await.len(),
        1,
        "a refresh without a change is not audited"
    );
}

#[tokio::test]
async fn compatible_reconnect_clears_incompatible_since() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let (reply, _) = reject_protocol(&hub, &ident, ProtocolVersion { major: 1, minor: 0 }).await;
    assert!(matches!(reply, Message::Reject(_)));

    let mut agent = FakeAgent::connect(hub.addr).await;
    agent.authenticate(&ident).await;
    let report = hub
        .store
        .agent_report(ident.agent)
        .await
        .unwrap()
        .expect("the compatible report is stored");
    assert_eq!(report.protocol, ReportedProtocol { major: 1, minor: 1 });
    assert_eq!(report.incompatible_since, None);
    assert_eq!(protocol_audit(&hub).await.len(), 2);
}

#[tokio::test]
async fn link_to_incompatible_host_is_unavailable() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let (reply, _) = reject_protocol(&hub, &ident, ProtocolVersion { major: 1, minor: 0 }).await;
    assert!(matches!(reply, Message::Reject(_)));
    let link = hub.server.link();
    let deadline = hub.clock.now() + SignedDuration::from_secs(5);
    assert_eq!(
        link.run_tool(ident.host, ReadToolCall::DockerPs { all: false }, deadline)
            .await,
        Err(LinkError::Incompatible)
    );
    assert_eq!(
        link.run_tool(
            HostId::new(),
            ReadToolCall::DockerPs { all: false },
            deadline
        )
        .await,
        Err(LinkError::NotConnected),
        "a host that never authenticated is only unknown"
    );

    // A row that says another protocol without a rejection is a prediction.
    let expected = enroll(&hub, "expected").await;
    hub.store
        .record_agent_report(
            expected.agent,
            &AuthenticatedReport {
                protocol: ReportedProtocol { major: 1, minor: 0 },
                release: "0.1.0".to_owned(),
                compatible: true,
            },
            hub.clock.now(),
        )
        .await
        .unwrap();
    assert_eq!(
        link.run_tool(
            expected.host,
            ReadToolCall::DockerPs { all: false },
            deadline
        )
        .await,
        Err(LinkError::NotConnected),
        "an expected incompatibility is not a finding"
    );
}

#[tokio::test]
async fn reenrolling_a_host_revokes_the_previous_agent() {
    let hub = Hub::start().await;
    let first = enroll(&hub, "nas").await;
    let second = enroll(&hub, "nas").await;
    assert_eq!(first.host, second.host);
    let old = hub.store.get_agent(first.agent).await.unwrap().unwrap();
    assert!(old.revoked_at.is_some());
    assert_eq!(
        hub.store
            .live_agent(first.host)
            .await
            .unwrap()
            .map(|a| a.agent),
        Some(second.agent)
    );
    let mut agent = FakeAgent::connect(hub.addr).await;
    assert_eq!(
        rejected(&agent.hello(first.agent, PROTOCOL_VERSION).await),
        Some(RejectReason::Revoked)
    );
}

#[tokio::test]
async fn first_frame_must_arrive_within_ten_seconds() {
    let hub = Hub::start().await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    let started = hub.clock.now();
    pump(&hub.clock, SignedDuration::from_millis(500), agent.closed()).await;
    assert!(hub.clock.now().duration_since(started) >= SignedDuration::from_secs(10));
}
