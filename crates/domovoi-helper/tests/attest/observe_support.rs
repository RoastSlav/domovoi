//! Observing a scripted host: the fake `ip`, `agent.toml` and the endpoints of a report.

use std::sync::Arc;

use domovoi_core::attest::{Cidr, RouteLines};
use domovoi_helper::attest::endpoints::{AgentTomlEndpoints, EndpointSource as _};
use domovoi_helper::attest::ip_source::IpSource;
use domovoi_helper::attest::observe::{Endpoints, ReadError, observe};
use domovoi_testkit::{FakeHostInspect, FakeProcessExecutor};

use crate::support::{IP, Script};

/// A host with the fake `ip` installed, `agent.toml` naming `hub`, and nothing else.
pub fn host_with_ip(hub: &str) -> FakeHostInspect {
    use domovoi_core::identity::{PathFacts, PathKind};
    FakeHostInspect::new()
        .path(
            IP,
            PathFacts {
                kind: Some(PathKind::File),
                symlink: false,
                mode: 0o755,
                uid: 0,
                gid: 0,
            },
        )
        .text(
            "/etc/domovoi/agent.toml",
            format!("[agent]\nhost_name = \"a\"\nhub_url = \"wss://{hub}:7421\"\n"),
        )
}

/// Observes `script` for the management network `198.51.100.0/24` and the hub `192.0.2.77`.
pub async fn observe_script(script: &Script) -> Result<RouteLines, ReadError> {
    let exec = Arc::new(FakeProcessExecutor::new());
    script.apply(&exec);
    observe_with(exec, "192.0.2.77", &["198.51.100.0/24"]).await
}

/// Observes whatever `exec` answers for a hub address and management networks.
pub async fn observe_with(
    exec: Arc<FakeProcessExecutor>,
    hub: &str,
    cidrs: &[&str],
) -> Result<RouteLines, ReadError> {
    let inspect = Arc::new(host_with_ip(hub));
    let source =
        AgentTomlEndpoints::load(&*inspect, std::path::Path::new("/etc/domovoi/agent.toml"));
    let endpoints = Endpoints {
        hub_addrs: source.hub_addrs()?,
        link_peers: source.link_peers()?,
        cidrs: cidrs
            .iter()
            .map(|c| Cidr::parse_config(c).expect("cidr"))
            .collect(),
    };
    observe(&IpSource::new(exec, inspect), &endpoints).await
}
