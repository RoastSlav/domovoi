//! Shared builders: vector files, typed route lines and the sample reports.

use core::net::IpAddr;

use domovoi_core::attest::{
    BootId, Cidr, CompleteReport, ConfigLines, EndpointRoute, Family, Hash32, IncompleteReason,
    IncompleteReport, LinkPeer, LocalAddr, Nonce32, Route, RouteLines, Rule, SafetyInput, Selector,
};
use jiff::Timestamp;

/// The boot id every sample uses.
pub const BOOT: &str = "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f";
/// The second boot id, for reboot cases.
pub const BOOT_B: &str = "0a0b0c0d-1111-4222-8333-444455556666";
/// The published test-only Ed25519 seed: the bytes 0 to 31. It protects nothing.
pub const TEST_SEED: [u8; 32] = {
    let mut seed = [0_u8; 32];
    let mut index = 0;
    while index < 32 {
        seed[index] = index as u8;
        index += 1;
    }
    seed
};
/// When the sample reports were computed.
pub const COMPUTED_AT: Timestamp = Timestamp::constant(1_700_000_000, 0);

pub fn vector_path(name: &str) -> String {
    format!("{}/tests/attest/vectors/{name}", env!("CARGO_MANIFEST_DIR"))
}

pub fn vector_bytes(name: &str) -> Vec<u8> {
    std::fs::read(vector_path(name)).unwrap_or_else(|err| panic!("vector {name}: {err}"))
}

pub fn vector_text(name: &str) -> String {
    String::from_utf8(vector_bytes(name)).expect("vectors are UTF-8")
}

pub fn ip(text: &str) -> IpAddr {
    text.parse().expect("an address")
}

pub fn cidr(text: &str) -> Cidr {
    Cidr::parse_report(text).expect("a canonical cidr")
}

pub fn boot() -> BootId {
    BootId::parse(BOOT).expect("a boot id")
}

pub fn nonce() -> Nonce32 {
    Nonce32::from_bytes([9; 32])
}

fn names(dev: &str) -> Vec<u8> {
    dev.as_bytes().to_vec()
}

pub fn local_addr(addr: &str, dev: &[u8]) -> String {
    LocalAddr {
        addr: Cidr::parse_ifaddr(addr).expect("an ifaddr"),
        ifname: dev.to_vec(),
    }
    .render()
}

pub fn route(
    family: Family,
    table: u32,
    dst: &str,
    gateway: Option<&str>,
    dev: &str,
    metric: u32,
) -> String {
    Route {
        family,
        table,
        dst: cidr(dst),
        gateway: gateway.map(ip),
        ifname: names(dev),
        metric,
    }
    .render()
}

pub fn rule(family: Family, priority: u32, selector: Selector, table: u32) -> String {
    Rule {
        family,
        priority,
        selector,
        table,
    }
    .render()
}

pub fn endpoint_route(
    endpoint: &str,
    table: u32,
    gateway: Option<&str>,
    dev: &str,
    src: &str,
) -> String {
    EndpointRoute {
        endpoint: ip(endpoint),
        table,
        gateway: gateway.map(ip),
        ifname: names(dev),
        src: ip(src),
    }
    .render()
}

pub fn link_peer(addr: &str, port: u16) -> String {
    LinkPeer { ip: ip(addr), port }.render()
}

/// Vector R1 (R2 with `table100_gateway` changed), given in a scrambled order on purpose.
pub fn r1_lines(table100_gateway: &str) -> RouteLines {
    let fwmark = Selector {
        fwmark: Some((1, 0xffff_ffff)),
        ..Selector::default()
    };
    let mut lines = RouteLines {
        hub_addr: vec!["2001:db8::10".into(), "192.168.1.10".into()],
        link_peer: vec![link_peer("192.168.1.10", 7443)],
        local_addr: vec![
            local_addr("2001:db8::20/64", b"eth0"),
            local_addr("192.168.1.20/24", b"eth0"),
            local_addr("10.8.0.2/24", b"wg0"),
        ],
        route: vec![
            route(Family::Inet, 254, "192.168.1.0/24", None, "eth0", 100),
            route(
                Family::Inet,
                254,
                "0.0.0.0/0",
                Some("192.168.1.1"),
                "eth0",
                100,
            ),
            route(
                Family::Inet,
                100,
                "0.0.0.0/0",
                Some(table100_gateway),
                "wg0",
                0,
            ),
            route(
                Family::Inet6,
                254,
                "::/0",
                Some("2001:db8::1"),
                "eth0",
                1024,
            ),
        ],
        rule: vec![
            rule(Family::Inet, 32766, Selector::default(), 254),
            rule(Family::Inet, 100, fwmark, 100),
            rule(Family::Inet, 0, Selector::default(), 255),
            rule(Family::Inet6, 32766, Selector::default(), 254),
        ],
        endpoint_route: vec![
            endpoint_route(
                "2001:db8::10",
                254,
                Some("2001:db8::1"),
                "eth0",
                "2001:db8::20",
            ),
            endpoint_route("192.168.1.10", 254, None, "eth0", "192.168.1.20"),
        ],
    };
    lines.normalise();
    lines
}

/// Vector R4: a name the kernel would refuse and a selector holding `iif` and `not`.
pub fn r4_lines() -> RouteLines {
    RouteLines {
        hub_addr: vec!["192.168.1.10".into()],
        local_addr: vec![local_addr("192.168.1.20/24", b"eth|0 x")],
        rule: vec![rule(
            Family::Inet,
            200,
            Selector {
                iif: Some(b"br-lan".to_vec()),
                not: true,
                ..Selector::default()
            },
            200,
        )],
        ..RouteLines::default()
    }
}

/// The sample configuration of the signed-report vectors (documentation ranges only).
pub fn sample_input() -> SafetyInput {
    SafetyInput {
        management_cidrs: vec!["192.0.2.0/24".into(), "2001:db8::/32".into()],
        protected_units: vec!["svc-a".into()],
        protected_containers: vec!["ctr-a".into()],
        max_ttl: Some("2h".into()),
        max_active: Some(5),
    }
}

/// The sample route state of the signed-report vectors.
pub fn sample_routes() -> RouteLines {
    let mut lines = RouteLines {
        hub_addr: vec!["192.0.2.10".into()],
        link_peer: Vec::new(),
        local_addr: vec![local_addr("192.0.2.20/24", b"eth0")],
        route: vec![
            route(
                Family::Inet,
                254,
                "0.0.0.0/0",
                Some("192.0.2.1"),
                "eth0",
                100,
            ),
            route(Family::Inet, 254, "192.0.2.0/24", None, "eth0", 100),
        ],
        rule: vec![
            rule(Family::Inet, 0, Selector::default(), 255),
            rule(Family::Inet, 32766, Selector::default(), 254),
        ],
        endpoint_route: vec![endpoint_route(
            "192.0.2.10",
            254,
            None,
            "eth0",
            "192.0.2.20",
        )],
    };
    lines.normalise();
    lines
}

pub fn complete_report() -> CompleteReport {
    let config = ConfigLines::effective(&sample_input()).expect("a valid sample");
    let routes = sample_routes();
    CompleteReport {
        helper_key_id: "0123456789abcdef".to_owned(),
        nonce: nonce(),
        boot_id: boot(),
        generation: 1,
        config_hash: config.hash(),
        route_state_hash: routes.hash(),
        computed_at: COMPUTED_AT,
        config,
        routes,
    }
}

pub fn incomplete_report() -> IncompleteReport {
    IncompleteReport {
        helper_key_id: "0123456789abcdef".to_owned(),
        nonce: nonce(),
        boot_id: boot(),
        reason: IncompleteReason::new(IncompleteReason::STATE_MISSING).expect("a token"),
        computed_at: COMPUTED_AT,
    }
}

pub fn hash(text: &str) -> Hash32 {
    text.parse().expect("a lowercase hash")
}

pub fn parse_uint_text(text: &str) -> Option<u64> {
    domovoi_core::attest::parse_uint(text).ok()
}
