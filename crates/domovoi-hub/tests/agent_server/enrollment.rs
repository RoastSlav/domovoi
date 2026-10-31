//! Certificates, enrollment tokens and the handshake.

use domovoi_core::ports::{Clock, HostRepo};
use domovoi_core::{AgentId, HostKind, HostName};
use domovoi_hub::agent_server::{AgentServer, AgentServerDeps, TlsIdentity, token_hash};
use domovoi_proto::messages::{Enrolled, RejectReason};
use domovoi_proto::{Message, PROTOCOL_VERSION, ProtocolVersion};
use jiff::SignedDuration;

use crate::agent::{FakeAgent, enroll, new_key, rejected};
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
            "domovoi agent enroll --hub wss://testhub:7421/ --token {} --fingerprint {}",
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

#[tokio::test]
async fn incompatible_major_version_is_rejected() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    let newer = ProtocolVersion {
        major: PROTOCOL_VERSION.major + 1,
        minor: 0,
    };
    let reply = agent.hello(ident.agent, newer).await;
    assert_eq!(
        rejected(&reply),
        Some(RejectReason::Incompatible {
            hub: PROTOCOL_VERSION
        })
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
