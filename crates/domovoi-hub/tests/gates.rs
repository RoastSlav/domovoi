//! Invariant gates for the hub. Never delete or weaken these; extend them.
//!
//! Built into the `agent_server` test binary, which owns the scripted agent.

use domovoi_core::ReadToolCall;
use domovoi_core::ports::{AgentLink, LinkError, Presence};
use domovoi_proto::{Message, PROTOCOL_VERSION};
use jiff::SignedDuration;

use crate::agent::{FakeAgent, enroll, new_key};
use crate::harness::{Hub, t0};

async fn reachable(hub: &Hub, host: domovoi_core::HostId) -> bool {
    let link = hub.server.link();
    let call = link
        .run_tool(
            host,
            ReadToolCall::DockerPs { all: false },
            t0() + SignedDuration::from_secs(30),
        )
        .await;
    assert!(
        matches!(link.presence(host), Presence::Connected { .. }) || call.is_err(),
        "presence and requests agree"
    );
    call != Err(LinkError::NotConnected)
}

/// INV-10: the link reaches an agent only after it signed a fresh challenge with its
/// enrolled key. Enrolling, saying hello and answering with the wrong key all leave the
/// host unreachable.
#[tokio::test]
async fn agent_link_requires_completed_handshake() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    assert!(
        !reachable(&hub, ident.host).await,
        "enrolling opens no session"
    );

    let mut halfway = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = halfway.hello(ident.agent, PROTOCOL_VERSION).await else {
        panic!("expected a challenge");
    };
    assert!(
        !reachable(&hub, ident.host).await,
        "hello alone opens no session"
    );

    let reply = halfway.answer(&challenge, ident.agent, &new_key()).await;
    assert!(matches!(reply, Message::Reject(_)));
    assert!(
        !reachable(&hub, ident.host).await,
        "a wrong key opens no session"
    );

    let mut agent = FakeAgent::connect(hub.addr).await;
    agent.authenticate(&ident).await;
    assert!(
        matches!(
            hub.server.link().presence(ident.host),
            Presence::Connected { .. }
        ),
        "the full handshake does"
    );
}
