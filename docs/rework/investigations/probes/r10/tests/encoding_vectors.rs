//! A1-A4: the 04b vectors. Bytes are built from the documented algorithm, never from the
//! published digests; the digests below are constants used only in assertions.

mod common;

use common::{first_diff, vec_file, vec_text};
use r10::config::effective_config;
use r10::encoding::{Cidr, Family};
use r10::fixture::cidr;
use r10::hash::{CONFIG_PREFIX, ROUTES_PREFIX, domain_bytes, sha256};
use r10::lines::{EndpointRoute, LinkPeer, LocalAddr, Route, Rule, Selector};
use r10::report::check_line;
use r10::routes::RouteLines;

use std::net::IpAddr;

const C1: &str = "96f1584f01f025fa82b1e57c3ebf8d220b87d6d88543d08add3dc273cbf4b9ea";
const C3: &str = "494a37a86856bb2b2f6e80e13efc654e8b7fbaf9e73245277f6d34993d42dc4f";
const C4: &str = "7f1db0e04eb12b4211166be055486a71e44be9e00c2a089cc9e51f4f06556baa";
const R1: &str = "082d1afdf657096d1b3a97215142239f6910b74f13f158fbdda7f4d9fd4d1515";
const R2: &str = "7eb46fcc67a00328df9e33578f42a4a890e6797223d6676a162c0b4ca1777ad9";
const R3: &str = "7947b31b2c89edab797c3f6a1251b42ca4452d64d06dd786d68570d721996ec8";
const R4: &str = "530d9a9e286ca63ee69147d0a7e0326bceb4199f114824f1969e4126e7ac925a";

fn config_bytes(input: &str) -> Vec<u8> {
    let c = effective_config(&vec_text(input)).expect("valid config");
    domain_bytes(CONFIG_PREFIX, &c.lines())
}

fn check(name: &str, produced: &[u8], fixture: &str, want_len: usize, want_digest: &str) {
    let expected = vec_file(fixture);
    let digest = hex::encode(sha256(produced));
    let ok = produced == expected.as_slice() && produced.len() == want_len && digest == want_digest;
    println!(
        "VECTOR {name} expected_len={want_len} actual_len={} expected_sha256={want_digest} actual_sha256={digest} result={}",
        produced.len(),
        if ok { "PASS" } else { "FAIL" }
    );
    assert_eq!(produced.len(), want_len, "{name}: length; {}", first_diff(produced, &expected));
    assert_eq!(produced, expected.as_slice(), "{name}: bytes; {}", first_diff(produced, &expected));
    assert_eq!(digest, want_digest, "{name}: digest");
}

#[test]
fn config_hash_matches_vectors() {
    check("C1", &config_bytes("c1_input.toml"), "c1.txt", 320, C1);
    check("C2", &config_bytes("c2_input.toml"), "c2.txt", 320, C1);
    check("C3", &config_bytes("c3_input.toml"), "c3.txt", 320, C3);
    check("C4", &config_bytes("c4_input.toml"), "c4.txt", 198, C4);
}

#[test]
fn equivalent_toml_same_config_hash() {
    let (a, b) = (vec_text("c1_input.toml"), vec_text("c2_input.toml"));
    assert_ne!(a.as_bytes(), b.as_bytes(), "inputs differ as text");
    let (ca, cb) = (effective_config(&a).unwrap(), effective_config(&b).unwrap());
    assert_eq!(ca, cb);
    assert_eq!(ca.hash(), cb.hash());
}

#[test]
fn narrowing_a_cidr_changes_the_hash_and_defaults_apply() {
    let c1 = effective_config(&vec_text("c1_input.toml")).unwrap();
    let c3 = effective_config(&vec_text("c3_input.toml")).unwrap();
    assert_ne!(c1.hash(), c3.hash());
    let c4 = effective_config(&vec_text("c4_input.toml")).unwrap();
    assert_eq!((c4.max_ttl_s, c4.max_active), (3600, 0));
    assert!(c4.cidr.is_empty() && c4.protected_container.is_empty());
    assert_eq!(c4.protected_unit.len(), 4, "only the mandatory set");
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn ifaddr(s: &str, dev: &str) -> String {
    LocalAddr { addr: r10::encoding::parse_ifaddr(s).unwrap(), ifname: dev.as_bytes().to_vec() }.render()
}

fn rt(fam: Family, table: u32, dst: &str, gw: Option<&str>, dev: &str, metric: u32) -> String {
    Route { family: fam, table, dst: cidr(dst), gateway: gw.map(ip), ifname: dev.as_bytes().to_vec(), metric }.render()
}

fn rl(fam: Family, prio: u32, sel: Selector, table: u32) -> String {
    Rule { family: fam, priority: prio, selector: sel, table }.render()
}

fn er(ep: &str, table: u32, gw: Option<&str>, dev: &str, src: &str) -> String {
    EndpointRoute { endpoint: ip(ep), table, gateway: gw.map(ip), ifname: dev.as_bytes().to_vec(), src: ip(src) }.render()
}

fn r1_lines(table100_gw: &str) -> RouteLines {
    let fw = Selector { fwmark: Some((1, 0xffff_ffff)), ..Selector::default() };
    // Deliberately given in a scrambled order: normalise() must reproduce the sorted order.
    let mut l = RouteLines {
        hub_addr: vec!["2001:db8::10".into(), "192.168.1.10".into()],
        link_peer: vec![LinkPeer { ip: ip("192.168.1.10"), port: 7443 }.render()],
        local_addr: vec![ifaddr("2001:db8::20/64", "eth0"), ifaddr("192.168.1.20/24", "eth0"), ifaddr("10.8.0.2/24", "wg0")],
        route: vec![
            rt(Family::Inet, 254, "192.168.1.0/24", None, "eth0", 100),
            rt(Family::Inet, 254, "0.0.0.0/0", Some("192.168.1.1"), "eth0", 100),
            rt(Family::Inet, 100, "0.0.0.0/0", Some(table100_gw), "wg0", 0),
            rt(Family::Inet6, 254, "::/0", Some("2001:db8::1"), "eth0", 1024),
        ],
        rule: vec![
            rl(Family::Inet, 32766, Selector::default(), 254),
            rl(Family::Inet, 100, fw, 100),
            rl(Family::Inet, 0, Selector::default(), 255),
            rl(Family::Inet6, 32766, Selector::default(), 254),
        ],
        endpoint_route: vec![
            er("2001:db8::10", 254, Some("2001:db8::1"), "eth0", "2001:db8::20"),
            er("192.168.1.10", 254, None, "eth0", "192.168.1.20"),
        ],
    };
    l.normalise();
    l
}

fn route_bytes(l: &RouteLines) -> Vec<u8> {
    domain_bytes(ROUTES_PREFIX, &l.lines())
}

#[test]
fn route_state_hash_matches_vectors() {
    check("R1", &route_bytes(&r1_lines("10.8.0.1")), "r1.txt", 583, R1);
    check("R2", &route_bytes(&r1_lines("10.8.0.254")), "r2.txt", 585, R2);
    check("R3", &route_bytes(&RouteLines::default()), "r3.txt", 25, R3);
    let r4 = RouteLines {
        hub_addr: vec!["192.168.1.10".into()],
        local_addr: vec![LocalAddr { addr: r10::encoding::parse_ifaddr("192.168.1.20/24").unwrap(), ifname: b"eth|0 x".to_vec() }.render()],
        rule: vec![rl(Family::Inet, 200, Selector { iif: Some("br-lan".into()), not: true, ..Selector::default() }, 200)],
        ..RouteLines::default()
    };
    check("R4", &route_bytes(&r4), "r4.txt", 123, R4);
}

#[test]
fn every_vector_line_passes_the_strict_line_grammar() {
    for name in ["r1.txt", "r2.txt", "r4.txt", "c1.txt", "c4.txt"] {
        for line in vec_text(name).lines().skip(1) {
            if line.starts_with("max_") {
                continue;
            }
            check_line(line).unwrap_or_else(|e| panic!("{name}: {line}: {e}"));
        }
    }
    let _ = Cidr::host(ip("10.0.0.1"));
}
