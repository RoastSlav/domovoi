//! What the hub tells the action executor about a host's agent: unknown until it has
//! connected in this hub run, known while it is connected, absent once its session ended,
//! and incompatible when this hub rejected its protocol.

use std::sync::Arc;

use domovoi_core::ports::{AgentLink, Clock, HostAvailability, HostRepo};
use domovoi_core::{AuthenticatedReport, HostName, ReportedProtocol};
use domovoi_hub::agent_server::{PolicyLink, SkillPolicy};
use domovoi_proto::{Message, ProtocolVersion};
use jiff::SignedDuration;

use crate::agent::{FakeAgent, Ident, enroll, online};
use crate::harness::{Hub, pump, t0};

/// Answers the challenge with the enrolled key and then waits for the hub to close the
/// connection, so the hub has judged and recorded `protocol` for this agent.
async fn rejected_protocol(hub: &Hub, ident: &Ident, protocol: ProtocolVersion) -> Message {
    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = agent.hello(ident.agent, protocol).await else {
        panic!("the hub asks every caller to prove its key first");
    };
    let reply = agent.answer(&challenge, ident.agent, &ident.key).await;
    agent.closed().await;
    reply
}

/// Polls `availability` until the teardown of the agent's session has run.
async fn settled(hub: &Hub, host: domovoi_core::HostId) -> HostAvailability {
    let link = hub.server.link();
    pump(&hub.clock, SignedDuration::from_millis(100), async {
        loop {
            if let HostAvailability::Absent { .. } = link.availability(host).await {
                return link.availability(host).await;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
}

#[tokio::test]
async fn availability_is_unknown_until_first_session_then_known_then_absent() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;

    // An enrolled agent with no session yet is unknown, not absent: it has never been
    // seen in this hub run.
    assert_eq!(
        hub.server.link().availability(ident.host).await,
        HostAvailability::Unknown,
    );

    let (_, agent) = online(&hub, "nas").await;
    let since = hub.clock.now();
    assert_eq!(
        hub.server.link().availability(ident.host).await,
        HostAvailability::Known { since },
        "a live session is known, since it started",
    );

    // The agent goes away; the hub's connection task ends the session at the hub's clock.
    drop(agent);
    assert!(
        matches!(
            settled(&hub, ident.host).await,
            HostAvailability::Absent { .. },
        ),
        "a host whose session ended is absent, not unknown",
    );
}

#[tokio::test]
async fn availability_is_unknown_after_hub_restart() {
    let hub = Hub::start().await;
    let (ident, agent) = online(&hub, "nas").await;
    drop(agent);
    settled(&hub, ident.host).await;

    // A second server over the same store and clock has no session of its own, so the
    // previous run's absence says nothing about this one.
    let restarted = hub.restart();
    assert_eq!(
        restarted.link().availability(ident.host).await,
        HostAvailability::Unknown,
        "a hub that just started has never seen the host",
    );
}

#[tokio::test]
async fn track_does_not_make_a_host_absent() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    // Watching known hosts records a `last_seen` without any session having existed.
    hub.server.watch_known_hosts().await.unwrap();
    assert!(
        matches!(
            hub.server.link().presence(ident.host),
            domovoi_core::ports::Presence::Disconnected { last_seen: Some(_) },
        ),
        "the host was tracked with a last-seen time",
    );
    assert_eq!(
        hub.server.link().availability(ident.host).await,
        HostAvailability::Unknown,
        "tracking a host is not the same as its agent having been connected",
    );
}

#[tokio::test]
async fn policy_link_forwards_availability() {
    let hub = Hub::start().await;
    let (ident, _agent) = online(&hub, "nas").await;
    let policy = SkillPolicy::load(hub.store.clone(), &[]).await.unwrap();
    let link = PolicyLink::wrap(hub.server.link(), Arc::new(policy));
    assert_eq!(
        link.availability(ident.host).await,
        hub.server.link().availability(ident.host).await,
        "the policy narrows skills, not whether the host is there",
    );
    assert!(
        matches!(
            link.availability(ident.host).await,
            HostAvailability::Known { .. },
        ),
        "the wrapped link still sees the live session",
    );
}

#[tokio::test]
async fn rejected_protocol_makes_host_incompatible() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let reply = rejected_protocol(&hub, &ident, ProtocolVersion { major: 1, minor: 0 }).await;
    assert!(matches!(reply, Message::Reject(_)), "{reply:?}");
    assert_eq!(
        hub.server.link().availability(ident.host).await,
        HostAvailability::Incompatible {
            reported: "1.0".to_owned(),
        },
        "an agent this hub rejected is incompatible, not merely gone",
    );
}

#[tokio::test]
async fn expected_incompatible_host_is_not_incompatible() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    // A stored report from an older hub is a prediction: this hub has not rejected it.
    hub.store
        .record_agent_report(
            ident.agent,
            &AuthenticatedReport {
                protocol: ReportedProtocol { major: 1, minor: 0 },
                release: "0.1.0".to_owned(),
                compatible: true,
            },
            t0(),
        )
        .await
        .unwrap();
    assert_eq!(
        hub.server.link().availability(ident.host).await,
        HostAvailability::Unknown,
        "an expected incompatibility does not tell the executor the host is unusable",
    );

    // Once the agent connects with a version this hub speaks, it is known again.
    let mut agent = FakeAgent::connect(hub.addr).await;
    agent.authenticate(&ident).await;
    assert!(
        matches!(
            hub.server.link().availability(ident.host).await,
            HostAvailability::Known { .. },
        ),
        "a compatible session is known despite the older stored report",
    );
}

#[tokio::test]
async fn a_new_session_makes_an_absent_host_known_again() {
    let hub = Hub::start().await;
    let (ident, agent) = online(&hub, "nas").await;
    drop(agent);
    settled(&hub, ident.host).await;

    let mut again = FakeAgent::connect(hub.addr).await;
    again.authenticate(&ident).await;
    assert!(
        matches!(
            hub.server.link().availability(ident.host).await,
            HostAvailability::Known { .. },
        ),
        "an agent that reconnects is known again",
    );

    // The host name is reused by the enrollment helper only through `online`, so a host
    // created but never connected stays unknown rather than absent.
    let never = HostName::parse("never").unwrap();
    let invite = hub.server.create_token(&never).await.unwrap();
    assert_eq!(
        hub.server.link().availability(invite.host).await,
        HostAvailability::Unknown,
    );
}
