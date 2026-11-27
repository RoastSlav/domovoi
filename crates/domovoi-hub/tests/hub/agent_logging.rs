//! What the hub logs when an agent is refused or leaves.

use domovoi_core::AgentId;
use domovoi_proto::PROTOCOL_VERSION;
use domovoi_proto::messages::RejectReason;
use domovoi_testkit::log_capture::LogCapture;
use jiff::SignedDuration;

use crate::agent::{FakeAgent, enroll, new_key, rejected};
use crate::harness::{Hub, pump};

#[tokio::test]
async fn a_wrong_enrollment_token_is_logged_with_the_peer_and_a_sentence() {
    let hub = Hub::start().await;
    let (logs, _guard) = LogCapture::start();
    let mut agent = FakeAgent::connect(hub.addr).await;
    let reply = pump(
        &hub.clock,
        SignedDuration::from_millis(100),
        agent.enroll("not-a-token", &new_key()),
    )
    .await;
    assert_eq!(rejected(&reply), Some(RejectReason::BadToken));
    agent.closed().await;
    let lines = logs.lines_with("agent handshake failed");
    assert_eq!(lines.len(), 1, "{}", logs.text());
    for field in ["WARN", "peer=127.0.0.1", "unknown, expired or already used"] {
        assert!(lines[0].contains(field), "lacks {field}: {}", lines[0]);
    }
    assert!(
        !lines[0].contains("BadToken"),
        "an enum name is not a reason"
    );
    assert!(
        !lines[0].contains("not-a-token"),
        "the token is never logged"
    );
}

#[tokio::test]
async fn a_revoked_agent_is_named_by_host_and_ids() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    assert!(hub.server.revoke_agent(ident.host).await.unwrap());
    let (logs, _guard) = LogCapture::start();
    let mut agent = FakeAgent::connect(hub.addr).await;
    let reply = agent.hello(ident.agent, PROTOCOL_VERSION).await;
    assert_eq!(rejected(&reply), Some(RejectReason::Revoked));
    agent.closed().await;
    let lines = logs.lines_with("agent handshake failed");
    assert_eq!(lines.len(), 1, "{}", logs.text());
    for field in [
        "host=nas".to_owned(),
        format!("host_id={}", ident.host),
        format!("agent_id={}", ident.agent),
        "revoked or replaced".to_owned(),
        "peer=127.0.0.1".to_owned(),
    ] {
        assert!(lines[0].contains(&field), "lacks {field}: {}", lines[0]);
    }
}

#[tokio::test]
async fn an_unknown_agent_is_refused_with_the_id_it_claimed() {
    let hub = Hub::start().await;
    let (logs, _guard) = LogCapture::start();
    let claimed = AgentId::new();
    let mut agent = FakeAgent::connect(hub.addr).await;
    let reply = agent.hello(claimed, PROTOCOL_VERSION).await;
    assert_eq!(rejected(&reply), Some(RejectReason::UnknownAgent));
    let line = logs.lines_with("agent handshake failed").remove(0);
    assert!(line.contains(&format!("agent_id={claimed}")), "{line}");
    assert!(line.contains("never enrolled here"), "{line}");
}
