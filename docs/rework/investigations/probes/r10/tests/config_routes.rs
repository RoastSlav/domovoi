//! E1-E2 (configuration) and F1-F4 (route state), all on synthetic fixtures.

mod common;

use common::{BOOT_A, Rig, T0, base_toml, env, ip, lan_snapshot};
use r10::config::{ConfigError, effective_config};
use r10::encoding::Family;
use r10::fixture::{FixtureSource, Snapshot, addr, base_rules, cidr, route, rule};
use r10::helper::{Inputs, compute};
use r10::lines::{LocalAddr, Rule, Selector};
use r10::routes::{Endpoints, ReadError, RouteLines, RuleAction, observe};

fn eps() -> Endpoints {
    Endpoints { hub_addrs: vec![ip("192.168.1.10")], link_peers: vec![(ip("192.168.1.10"), 7443)], cidrs: vec![cidr("192.168.1.0/24"), cidr("2001:db8::/32")] }
}

fn obs(s: &Snapshot) -> RouteLines {
    observe(&FixtureSource(s.clone()), &eps()).expect("observe")
}

fn generation(rig: &Rig, now: u64) -> u64 {
    let r = rig.attest_raw([4; 32], now, BOOT_A).unwrap().report;
    String::from_utf8(r).unwrap().lines().find_map(|l| l.strip_prefix("generation=")).unwrap().parse().unwrap()
}

#[test]
fn effective_configuration_is_normalised_deduplicated_and_defaulted() {
    let c = effective_config("[safety]\nmanagement_cidrs=[\"10.1.2.3/8\",\"10.0.0.0/8\",\"2001:DB8:0:0:0:0:0:1/32\"]\nprotected_units=[\"plex\",\"plex.service\",\"Plex\",\"getty@tty1\",\"foo.bar\"]\nprotected_containers=[\"a\",\"a\"]\n").unwrap();
    assert_eq!(c.cidr, vec!["10.0.0.0/8", "2001:db8::/32"]);
    for u in ["plex.service", "Plex.service", "getty@tty1.service", "foo.bar.service"] {
        assert!(c.protected_unit.contains(&u.to_owned()), "{u}");
    }
    assert_eq!(c.protected_unit.iter().filter(|u| *u == "plex.service").count(), 1);
    assert_eq!(c.protected_container, vec!["a"]);
    assert_eq!((c.max_ttl_s, c.max_active), (3600, 0));
    // The mandatory set cannot be removed by configuration.
    let c = effective_config("[safety]\nprotected_units=[]\n").unwrap();
    assert_eq!(c.protected_unit.len(), 4);
    let d = |t: &str| effective_config(&format!("[safety.containment]\nmax_ttl=\"{t}\"\n")).unwrap().max_ttl_s;
    assert_eq!((d("24h"), d("1440m"), d("86400s"), d("1d")), (86400, 86400, 86400, 86400));
    // No [safety] table at all: same as empty (harness reading).
    assert_eq!(effective_config("groups=[\"net.read\"]\n").unwrap(), effective_config("[safety]\n").unwrap());
}

#[test]
fn invalid_configuration_yields_no_hash_and_config_invalid() {
    let cases = [
        ("invalid_toml", "[safety\n"),
        ("cidr_prefix_too_long", "[safety]\nmanagement_cidrs=[\"10.0.0.0/33\"]\n"),
        ("cidr_without_prefix", "[safety]\nmanagement_cidrs=[\"10.0.0.1\"]\n"),
        ("cidr_garbage", "[safety]\nmanagement_cidrs=[\"x/8\"]\n"),
        ("unit_name_with_space", "[safety]\nprotected_units=[\"bad name\"]\n"),
        ("container_leading_dash", "[safety]\nprotected_containers=[\"-x\"]\n"),
        ("unknown_key", "[safety]\nmanagement_cidr=[\"10.0.0.0/8\"]\n"),
        ("bad_duration", "[safety.containment]\nmax_ttl=\"1x\"\n"),
        ("negative_active", "[safety.containment]\nmax_active=-1\n"),
        ("wrong_type", "[safety]\nmanagement_cidrs=\"10.0.0.0/8\"\n"),
    ];
    for (name, toml) in cases {
        let e = effective_config(toml).expect_err(name);
        println!("CONFIG-INVALID {name}: {e}");
        assert_eq!(e.reason(), "config_invalid");
        let mut rig = Rig::new();
        rig.toml = toml.to_owned();
        let rep = String::from_utf8(rig.attest_raw([1; 32], T0, BOOT_A).unwrap().report).unwrap();
        assert!(rep.contains("complete=false\nincomplete_reason=config_invalid\n"), "{name}");
        assert!(rep.contains(&format!("config_hash={}\n", "0".repeat(64))), "{name}: sentinel instead of a hash");
    }
    assert!(matches!(effective_config("[safety\n"), Err(ConfigError::Parse(_))));
}

#[test]
fn multiple_tables_and_rules_observed() {
    let l = obs(&lan_snapshot());
    assert!(l.rule.contains(&"inet|100|fwmark%3D0x1/0xffffffff|100".to_owned()));
    assert!(l.route.contains(&"inet|100|0.0.0.0/0|10.8.0.1|wg0|0".to_owned()), "default route of a referenced non-main table");
    assert!(l.route.contains(&"inet|254|192.168.1.0/24|-|eth0|100".to_owned()), "route a lookup selected");
    assert_eq!(l.endpoint_route.len(), 3, "hub, network address of each cidr: {:?}", l.endpoint_route);
    assert!(l.endpoint_route.iter().any(|e| e.starts_with("2001:db8::|")));
    assert_eq!(l.rule.len(), 5, "v4: 0, 100, 32766; v6: 0, 32766");
}

#[test]
fn multiple_interfaces_and_gateways_observed() {
    let mut s = lan_snapshot();
    s.routes.push(route(254, "10.20.0.0/24", Some("10.20.0.1"), "eth1", 200));
    s.addrs.push(addr("10.20.0.7/24", "eth1"));
    let mut e = eps();
    e.link_peers.push((ip("10.20.0.5"), 9000));
    let l = observe(&FixtureSource(s), &e).unwrap();
    assert!(l.endpoint_route.contains(&"10.20.0.5|254|10.20.0.1|eth1|10.20.0.7".to_owned()));
    assert!(l.local_addr.contains(&"10.20.0.7/24|eth1".to_owned()));
    assert!(l.local_addr.iter().any(|a| a.ends_with("|eth0")));
    assert!(!l.local_addr.iter().any(|a| a.ends_with("docker0")), "unrelated interface not attested");
}

#[test]
fn vpn_tunnel_route_to_hub_observed() {
    let mut s = lan_snapshot();
    s.routes.push(route(254, "10.99.0.0/24", None, "wg0", 0));
    let mut e = eps();
    e.hub_addrs = vec![ip("10.99.0.1")];
    e.link_peers = vec![(ip("10.99.0.1"), 7443)];
    let l = observe(&FixtureSource(s.clone()), &e).unwrap();
    assert!(l.endpoint_route.contains(&"10.99.0.1|254|-|wg0|10.8.0.2".to_owned()));
    assert!(l.local_addr.contains(&"10.8.0.2/24|wg0".to_owned()), "tunnel address attested because an endpoint route names wg0");
    // Replacing the tunnel route (same prefix, now via eth0) changes the attested state.
    let mut s2 = s.clone();
    s2.routes.retain(|r| r.dst != cidr("10.99.0.0/24"));
    s2.routes.push(route(254, "10.99.0.0/24", Some("192.168.1.1"), "eth0", 0));
    assert_ne!(observe(&FixtureSource(s2), &e).unwrap().hash(), l.hash());
}

#[test]
fn nondefault_route_change_for_endpoint_changes_generation() {
    let mut rig = Rig::new();
    let g0 = generation(&rig, T0);
    assert_eq!(generation(&rig, T0 + 1), g0, "no change, no increment");
    for r in &mut rig.snap.routes {
        if r.dst == cidr("192.168.1.0/24") {
            r.metric = 50;
        }
    }
    assert_eq!(generation(&rig, T0 + 2), g0 + 1);
}

#[test]
fn irrelevant_route_change_keeps_generation() {
    let mut rig = Rig::new();
    let g0 = generation(&rig, T0);
    rig.snap.routes.push(route(254, "172.30.0.0/16", None, "docker0", 0));
    rig.snap.addrs.push(addr("172.30.0.1/16", "docker0"));
    rig.snap.routes.push(route(200, "10.77.0.0/16", None, "eth0", 0)); // table no rule references
    assert_eq!(generation(&rig, T0 + 1), g0);
    // Finding: a more specific route inside a configured CIDR is not attested either, because
    // only the CIDR's network address is looked up.
    rig.snap.routes.push(route(254, "192.168.1.128/25", Some("192.168.1.99"), "eth0", 5));
    assert_eq!(generation(&rig, T0 + 2), g0, "cidr interior change invisible");
}

#[test]
fn route_replacement_and_deletion_detected() {
    let mut rig = Rig::new();
    let g0 = generation(&rig, T0);
    // Replacement: default gateway of main changes.
    for r in &mut rig.snap.routes {
        if r.table == 254 && r.dst == cidr("::/0") {
            r.gateway = Some(ip("2001:db8::2"));
        }
    }
    assert_eq!(generation(&rig, T0 + 1), g0 + 1);
    // Deletion of the selected on-link route: the lookup now falls to the default route.
    rig.snap.routes.retain(|r| r.dst != cidr("192.168.1.0/24"));
    assert_eq!(generation(&rig, T0 + 2), g0 + 2);
    // Deleting everything a lookup needs makes the read incomplete, not a new hash.
    rig.snap.routes.clear();
    let rep = String::from_utf8(rig.attest_raw([4; 32], T0 + 3, BOOT_A).unwrap().report).unwrap();
    assert!(rep.contains("complete=false\nincomplete_reason=lookup_failed\n"));
}

#[test]
fn incomplete_route_data_fails_closed() {
    type Edit = Box<dyn Fn(&mut Snapshot)>;
    let cases: Vec<(&str, &str, Edit)> = vec![
        ("rules_unreadable", "rules_unreadable", Box::new(|s| s.failures.rules = true)),
        ("table_unreadable", "table_unreadable", Box::new(|s| s.failures.tables.push(100))),
        ("partial_table_read", "route_read_partial", Box::new(|s| s.failures.partial_table = Some(254))),
        ("addresses_unreadable", "addresses_unreadable", Box::new(|s| s.failures.addrs = true)),
        ("lookup_failed", "lookup_failed", Box::new(|s| s.failures.lookups.push(ip("192.168.1.10")))),
        ("blackhole_rule", "rule_unrepresentable", Box::new(|s| {
            let mut r = rule(Family::Inet, 50, Selector::default(), 0);
            r.action = RuleAction::Other("blackhole".into());
            s.rules.push(r);
        })),
    ];
    for (name, token, edit) in cases {
        let mut s = lan_snapshot();
        edit(&mut s);
        let err: ReadError = observe(&FixtureSource(s.clone()), &eps()).expect_err(name);
        assert_eq!(err.reason(), token, "{name}");
        let mut rig = Rig::new();
        rig.snap = s;
        let rep = String::from_utf8(rig.attest_raw([4; 32], T0, BOOT_A).unwrap().report).unwrap();
        assert!(rep.contains(&format!("complete=false\nincomplete_reason={token}\n")), "{name}: {rep}");
        assert!(rep.contains(&format!("route_state_hash={}\n", "0".repeat(64))), "{name}: no route hash is produced");
        println!("INCOMPLETE {name}: incomplete_reason={token}");
    }
    let c = compute(&Inputs { config_text: Some(&base_toml()), routes: &FixtureSource(Snapshot { failures: r10::fixture::Failures { rules: true, ..Default::default() }, ..lan_snapshot() }), env: &env(T0, BOOT_A) });
    assert!(c.config.is_ok() && c.routes.is_err());
}

fn r1_snapshot() -> Snapshot {
    let mut rules = vec![rule(Family::Inet, 0, Selector::default(), 255)];
    rules.push(rule(Family::Inet, 100, Selector { fwmark: Some((1, 0xffff_ffff)), ..Selector::default() }, 100));
    rules.push(rule(Family::Inet, 32766, Selector::default(), 254));
    rules.push(rule(Family::Inet6, 32766, Selector::default(), 254));
    Snapshot {
        rules,
        routes: vec![
            route(254, "0.0.0.0/0", Some("192.168.1.1"), "eth0", 100),
            route(254, "192.168.1.0/24", None, "eth0", 100),
            route(100, "0.0.0.0/0", Some("10.8.0.1"), "wg0", 0),
            route(254, "::/0", Some("2001:db8::1"), "eth0", 1024),
        ],
        addrs: vec![addr("10.8.0.2/24", "wg0"), addr("192.168.1.20/24", "eth0"), addr("2001:db8::20/64", "eth0")],
        ..Snapshot::default()
    }
}

#[test]
fn r1_vector_is_an_encoding_vector_not_a_semantic_output_of_the_selection_rule() {
    let e = Endpoints { hub_addrs: vec![ip("192.168.1.10"), ip("2001:db8::10")], link_peers: vec![(ip("192.168.1.10"), 7443)], cidrs: vec![] };
    let got = observe(&FixtureSource(r1_snapshot()), &e).unwrap();
    let published = common::vec_text("r1.txt");
    let want: std::collections::BTreeSet<&str> = published.lines().skip(1).collect();
    let have_lines = got.lines();
    let have: std::collections::BTreeSet<&str> = have_lines.iter().map(String::as_str).collect();
    let only_published: Vec<_> = want.difference(&have).collect();
    let only_observed: Vec<_> = have.difference(&want).collect();
    println!("R1 only in published vector: {only_published:?}; only in observed output: {only_observed:?}");
    assert_eq!(only_published, vec![&"local_addr=10.8.0.2/24|wg0"], "wg0 is named by no endpoint_route");
    assert!(only_observed.is_empty());
    // The rule line for table 255 is present although no route of table 255 is attested: the
    // vector therefore reads "rule" as every lookup rule, which is the reading the harness uses.
    assert!(published.contains("rule=inet|0|all|255"));
}

#[test]
fn selector_encoding_is_ambiguous_for_interface_names_containing_separators() {
    // Finding: item values inside the selector are not escaped individually, so a (legal) kernel
    // interface name "a;not" and the pair iif=a + `not` render to the same bytes.
    let a = Selector { iif: Some("a;not".into()), ..Selector::default() };
    let b = Selector { iif: Some("a".into()), not: true, ..Selector::default() };
    assert_ne!(a, b);
    let (ra, rb) = (Rule { family: Family::Inet, priority: 1, selector: a, table: 1 }.render(), Rule { family: Family::Inet, priority: 1, selector: b.clone(), table: 1 }.render());
    assert_eq!(ra, rb);
    assert_eq!(Rule::parse(&ra).unwrap().selector, b, "the parser reads it as the `not` form");
    let _ = (LocalAddr::parse("10.0.0.1/8|eth0").unwrap(), base_rules(Family::Inet));
}
