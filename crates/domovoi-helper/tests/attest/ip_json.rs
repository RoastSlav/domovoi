use core::net::IpAddr;

use domovoi_core::attest::{Cidr, Family};
use domovoi_helper::attest::ip_json::{parse_addresses, parse_lookup, parse_routes, parse_rules};
use domovoi_helper::attest::observe::{ReadError, RouteKind, RuleAction};

use crate::support::fixture;

fn cidr(text: &str) -> Cidr {
    Cidr::parse_config(text).expect("cidr")
}

fn ip(text: &str) -> IpAddr {
    text.parse().expect("ip")
}

#[test]
fn ip_json_parsers_match_captured_fixtures() {
    let rules = parse_rules(&fixture("rules_v4_policy"), Family::Inet).expect("rules");
    assert_eq!(rules.len(), 5);
    assert_eq!(rules[1].table, 100);
    assert_eq!(rules[1].selector.from, Some(cidr("192.0.2.0/24")));
    assert_eq!(rules[1].selector.fwmark, Some((0x10, 0xff)));
    assert_eq!(rules[2].selector.to, Some(cidr("198.51.100.0/24")));
    assert_eq!(rules[2].selector.iif.as_deref(), Some(&b"dummy0"[..]));
    assert_eq!(rules[0].selector.render(), "all");

    let rules = parse_rules(&fixture("rules_v4_selectors"), Family::Inet).expect("rules");
    let complex = &rules[1];
    assert!(complex.selector.not);
    assert_eq!(complex.selector.from, Some(cidr("203.0.113.0/24")));
    assert_eq!(complex.selector.oif.as_deref(), Some(&b"dummy0"[..]));
    assert_eq!(complex.selector.ipproto, Some(6));
    assert_eq!(complex.selector.sport, Some((1000, 2000)));
    assert_eq!(complex.selector.dport, Some((443, 443)));
    assert_eq!(complex.selector.uidrange, Some((0, 0)));
    assert_eq!(rules[2].selector.fwmark, Some((0xca6c, u32::MAX)));
    assert_eq!(rules[2].table, 51820);

    let rules = parse_rules(&fixture("rules_v4_names"), Family::Inet).expect("rules");
    assert_eq!(rules[1].selector.oif.as_deref(), Some(&b"a=b"[..]));
    assert_eq!(rules[2].selector.iif.as_deref(), Some(&b"br|x"[..]));
    let rules = parse_rules(&fixture("rules_v6_names"), Family::Inet6).expect("rules");
    assert_eq!(rules[1].selector.iif.as_deref(), Some(&b"a;not"[..]));

    let routes = parse_routes(&fixture("routes_main_v4"), Family::Inet, 254).expect("routes");
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].dst.prefix, 0);
    assert_eq!(routes[0].gateway, Some(ip("192.0.2.1")));
    assert_eq!(routes[0].metric, 0);
    assert_eq!(routes[0].kind, RouteKind::Unicast);
    assert_eq!(routes[1].dst, cidr("192.0.2.0/24"));
    let routes = parse_routes(&fixture("routes_main_v6"), Family::Inet6, 254).expect("routes");
    assert_eq!(routes[2].metric, 1024);
    assert_eq!(routes[2].dst.prefix, 0);
    let local = parse_routes(&fixture("routes_local_v4"), Family::Inet, 255).expect("routes");
    assert_eq!(local[1].dst, Cidr::host(ip("127.0.0.1")));
    assert_eq!(local[0].kind, RouteKind::Other("2".to_owned()));

    let lookup = parse_lookup(&fixture("get_unicast_v4"), ip("198.51.100.7")).expect("lookup");
    assert_eq!(lookup.table, 254);
    assert_eq!(lookup.src, ip("192.0.2.10"));
    assert_eq!(lookup.ifname, b"dummy0");
    assert_eq!(lookup.kind, RouteKind::Unicast);

    let addresses = parse_addresses(&fixture("addresses")).expect("addresses");
    assert!(
        addresses.iter().any(|a| a.ifname == b"dummy0"
            && a.addr == Cidr::parse_ifaddr("192.0.2.10/24").expect("ifaddr"))
    );
    assert!(
        addresses
            .iter()
            .any(|a| a.addr.addr.to_string() == "2001:db8::10")
    );
}

#[test]
fn type_and_table_arrive_as_strings_or_numbers_and_unicast_is_one_or_absent() {
    for kind in [r#""type":"unicast","#, r#""type":"1","#, r#""type":1,"#, ""] {
        let json = format!(
            r#"[{{{kind}"dst":"192.0.2.9","dev":"eth0","table":254,"prefsrc":"192.0.2.1"}}]"#
        );
        let lookup = parse_lookup(&json, ip("192.0.2.9"));
        if kind.is_empty() {
            assert!(
                matches!(lookup, Err(ReadError::Partial(_))),
                "a lookup must name its type"
            );
        } else {
            assert_eq!(lookup.expect("parses").kind, RouteKind::Unicast, "{kind}");
        }
    }
    let local =
        r#"[{"type":"2","dst":"192.0.2.9","dev":"lo","table":"254","prefsrc":"192.0.2.9"}]"#;
    assert_eq!(
        parse_lookup(local, ip("192.0.2.9")).expect("parses").kind,
        RouteKind::Other("2".to_owned())
    );
}

#[test]
fn rules_with_an_action_other_than_lookup_are_returned_for_refusal() {
    for (name, key) in [
        ("rules_v4_blackhole", "action"),
        ("rules_v4_goto", "goto"),
        ("rules_v4_suppress", "suppress_prefixlen"),
    ] {
        let rules = parse_rules(&fixture(name), Family::Inet).expect("rules");
        assert!(
            rules
                .iter()
                .any(|r| r.action == RuleAction::Other(key.to_owned())),
            "{name}"
        );
    }
}

#[test]
fn unknown_rule_key_is_unrepresentable_not_ignored() {
    let json = r#"[{"priority":5,"src":"all","table":"254","novel_key":1}]"#;
    assert!(matches!(
        parse_rules(json, Family::Inet),
        Err(ReadError::Unrepresentable(_))
    ));
    let tos = r#"[{"priority":108,"src":"192.0.2.99","tos":"0x10","table":"100"}]"#;
    assert!(matches!(
        parse_rules(tos, Family::Inet),
        Err(ReadError::Unrepresentable(_))
    ));
    let partial_mask =
        r#"[{"priority":9,"src":"all","dport":443,"dport_mask":"0xff00","table":"254"}]"#;
    assert!(matches!(
        parse_rules(partial_mask, Family::Inet),
        Err(ReadError::Unrepresentable(_))
    ));
}

#[test]
fn malformed_json_and_missing_keys_are_partial_reads() {
    for json in [
        "",
        "{",
        "[1]",
        r#"{"a":1}"#,
        r#"[{"priority":1,"src":"all"}]"#,
    ] {
        assert!(
            matches!(parse_rules(json, Family::Inet), Err(ReadError::Partial(_))),
            "{json}"
        );
    }
    assert!(matches!(
        parse_routes(
            r#"[{"gateway":"192.0.2.1","dev":"eth0"}]"#,
            Family::Inet,
            254
        ),
        Err(ReadError::Partial(_))
    ));
    assert!(matches!(
        parse_routes(
            r#"[{"dst":"default","gateway":"192.0.2.1"}]"#,
            Family::Inet,
            254
        ),
        Err(ReadError::Partial(_))
    ));
}
