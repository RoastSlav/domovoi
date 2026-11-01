//! Silent agents: agent down, host down, and the hub doubting its own network first.

use std::sync::atomic::Ordering;

use domovoi_core::{Severity, SignalClass, SignalStatus, SourceKind};
use domovoi_proto::Message;
use domovoi_proto::messages::Ping;
use jiff::SignedDuration;

use crate::agent::online;
use crate::harness::{Hub, t0};

const SILENT: SignedDuration = SignedDuration::from_secs(31);

#[tokio::test]
async fn silent_agent_with_reachable_host_raises_agent_down() {
    let hub = Hub::start().await;
    let (ident, mut agent) = online(&hub, "nas").await;
    hub.server.check_presence().await;
    assert!(hub.signals().is_empty(), "a fresh session is not silent");

    hub.clock.advance(SILENT);
    hub.server.check_presence().await;
    let signals = hub.signals();
    assert_eq!(signals.len(), 1);
    let down = &signals[0];
    assert_eq!(down.check().as_str(), "agent.down");
    assert_eq!(down.host(), ident.host);
    assert_eq!(down.severity(), Severity::Warning);
    assert_eq!(down.source(), &SourceKind::Probe);
    assert!(
        down.summary().contains("the host answers"),
        "{}",
        down.summary()
    );
    assert_eq!(hub.probes.asked(), ["1.1.1.1:53", "127.0.0.1:22"]);

    hub.server.check_presence().await;
    assert_eq!(hub.signals().len(), 1, "raised once per silence");

    // Any frame from the agent resolves it.
    agent.send(Message::Ping(Ping { at: t0() }), None).await;
    agent.expect().await;
    hub.server.check_presence().await;
    let signals = hub.signals();
    assert_eq!(signals.len(), 2);
    assert_eq!(signals[1].check().as_str(), "agent.down");
    assert_eq!(signals[1].status(), SignalStatus::Resolved);
}

#[tokio::test]
async fn silent_agent_with_unreachable_host_raises_host_down() {
    let hub = Hub::start().await;
    let (ident, agent) = online(&hub, "nas").await;
    drop(agent);
    hub.probes.hosts_up.store(false, Ordering::SeqCst);
    hub.clock.advance(SILENT);
    hub.server.check_presence().await;
    let signals = hub.signals();
    assert_eq!(signals.len(), 1);
    let down = &signals[0];
    assert_eq!(down.check().as_str(), "host.down");
    assert_eq!(down.host(), ident.host);
    assert_eq!(down.severity(), Severity::Critical);
    assert_eq!(down.class(), SignalClass::Immediate);
    assert_eq!(
        hub.probes.asked(),
        ["1.1.1.1:53", "127.0.0.1:22", "127.0.0.1:443"]
    );
}

#[tokio::test]
async fn hub_network_down_raises_hub_network_degraded_and_no_host_down() {
    let hub = Hub::start().await;
    let (_ident, agent) = online(&hub, "nas").await;
    drop(agent);
    hub.probes.gateway.store(false, Ordering::SeqCst);
    hub.probes.hosts_up.store(false, Ordering::SeqCst);
    hub.clock.advance(SILENT);
    hub.server.check_presence().await;
    hub.server.check_presence().await;
    let signals = hub.signals();
    assert_eq!(signals.len(), 1, "{signals:?}");
    assert_eq!(signals[0].check().as_str(), "hub.network_degraded");
    assert_eq!(signals[0].host(), hub.hub_host);

    // The network comes back: the hub resolves its own signal, then looks at the host.
    hub.probes.gateway.store(true, Ordering::SeqCst);
    hub.server.check_presence().await;
    let checks: Vec<(String, SignalStatus)> = hub
        .signals()
        .iter()
        .map(|s| (s.check().as_str().to_owned(), s.status()))
        .collect();
    assert_eq!(
        checks,
        [
            ("hub.network_degraded".to_owned(), SignalStatus::Firing),
            ("hub.network_degraded".to_owned(), SignalStatus::Resolved),
            ("host.down".to_owned(), SignalStatus::Firing),
        ]
    );
}

#[tokio::test]
async fn configured_probes_decide_for_hosts_and_known_hosts_are_watched() {
    let hub = Hub::start_with(|s| {
        s.probes.insert(
            domovoi_core::HostName::parse("nas").unwrap(),
            vec!["nas.lan:8080".to_owned()],
        );
    })
    .await;
    // Enrolled but never connected since the hub started.
    let ident = crate::agent::enroll(&hub, "nas").await;
    hub.server.watch_known_hosts().await.unwrap();
    hub.clock.advance(SILENT);
    hub.server.check_presence().await;
    let signals = hub.signals();
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].host(), ident.host);
    assert_eq!(hub.probes.asked(), ["1.1.1.1:53", "nas.lan:8080"]);
}
