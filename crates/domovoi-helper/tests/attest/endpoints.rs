use std::path::Path;

use domovoi_helper::attest::endpoints::{AgentTomlEndpoints, EndpointSource};
use domovoi_helper::attest::observe::ReadError;
use domovoi_testkit::FakeHostInspect;

const AGENT_TOML: &str = "/etc/domovoi/agent.toml";

fn endpoints(host: &FakeHostInspect) -> AgentTomlEndpoints {
    AgentTomlEndpoints::load(host, Path::new(AGENT_TOML))
}

fn toml(url: &str) -> String {
    format!("[agent]\nhost_name = \"a\"\nhub_url = \"{url}\"\n")
}

#[test]
fn hub_url_literal_ip_becomes_hub_addr() {
    let host = FakeHostInspect::new().text(AGENT_TOML, toml("wss://192.0.2.7:7421"));
    assert_eq!(
        endpoints(&host).hub_addrs(),
        Ok(vec!["192.0.2.7".parse().expect("ip")])
    );
    let host = FakeHostInspect::new().text(AGENT_TOML, toml("wss://[2001:db8::7]:7421"));
    assert_eq!(
        endpoints(&host).hub_addrs(),
        Ok(vec!["2001:db8::7".parse().expect("ip")])
    );
}

#[test]
fn hub_url_hostname_yields_no_hub_addr_line() {
    let host = FakeHostInspect::new().text(AGENT_TOML, toml("wss://hub.example.net:7421"));
    assert_eq!(endpoints(&host).hub_addrs(), Ok(Vec::new()));
}

#[test]
fn link_peer_lines_are_empty_in_this_release() {
    let host = FakeHostInspect::new().text(AGENT_TOML, toml("wss://192.0.2.7:7421"));
    assert_eq!(endpoints(&host).link_peers(), Ok(Vec::new()));
}

#[test]
fn an_unreadable_agent_configuration_fails_every_read() {
    let missing = FakeHostInspect::new();
    let broken = FakeHostInspect::new().text(AGENT_TOML, "not toml [");
    for host in [&missing, &broken] {
        let source = endpoints(host);
        assert_eq!(source.hub_addrs(), Err(ReadError::ConfigUnreadable));
        assert_eq!(source.link_peers(), Err(ReadError::ConfigUnreadable));
    }
}
