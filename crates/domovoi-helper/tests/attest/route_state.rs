use std::sync::Arc;

use domovoi_core::ports::ProcessOutput;
use domovoi_helper::attest::observe::ReadError;
use domovoi_testkit::FakeProcessExecutor;

use crate::observe_support::{observe_script, observe_with};
use crate::support::{IP, Script, fixture};

async fn hash_of(script: &Script) -> String {
    let lines = observe_script(script).await.expect("observed");
    lines.hash().to_string()
}

fn via_table_100(script: &mut Script) {
    script.rules4 = fixture("rules_v4_policy");
    script.tables.insert(
        100,
        (fixture("routes_table100_v4"), fixture("routes_table100_v6")),
    );
    script.lookups.insert(
        "198.51.100.0".to_owned(),
        r#"[{"type":"1","dst":"198.51.100.0","gateway":"192.0.2.2","dev":"dummy0","table":"100","prefsrc":"192.0.2.10","flags":[],"uid":0,"cache":[]}]"#
            .to_owned(),
    );
}

#[tokio::test]
async fn a_stock_host_observes_its_default_routes_endpoints_and_addresses() {
    let lines = observe_script(&Script::stock()).await.expect("observed");
    assert_eq!(lines.hub_addr, ["192.0.2.77"]);
    assert!(lines.link_peer.is_empty());
    assert_eq!(
        lines.endpoint_route,
        [
            "192.0.2.77|254|-|dummy0|192.0.2.10",
            "198.51.100.0|254|192.0.2.1|dummy0|192.0.2.10"
        ]
    );
    assert!(
        lines
            .route
            .contains(&"inet|254|0.0.0.0/0|192.0.2.1|dummy0|0".to_owned())
    );
    assert!(
        lines
            .route
            .contains(&"inet6|254|::/0|2001:db8::1|dummy0|1024".to_owned())
    );
    assert!(
        lines
            .local_addr
            .contains(&"192.0.2.10/24|dummy0".to_owned())
    );
    assert_eq!(lines.rule.len(), 5, "three IPv4 rules and two IPv6 rules");
}

#[tokio::test]
async fn nondefault_route_change_for_endpoint_changes_generation() {
    let before = hash_of(&Script::stock()).await;
    let mut changed = Script::stock();
    via_table_100(&mut changed);
    assert_ne!(
        hash_of(&changed).await,
        before,
        "the selected route moved to table 100"
    );
    // The same endpoint through another gateway of the same table is a change too.
    let mut regated = changed.clone();
    for json in regated.lookups.values_mut() {
        *json = json.replace("\"192.0.2.2\"", "\"192.0.2.3\"");
    }
    regated.tables.insert(
        100,
        (
            fixture("routes_table100_v4").replace("\"192.0.2.2\"", "\"192.0.2.3\""),
            fixture("routes_table100_v6"),
        ),
    );
    assert_ne!(hash_of(&regated).await, hash_of(&changed).await);
}

#[tokio::test]
async fn irrelevant_route_change_keeps_generation() {
    let before = hash_of(&Script::stock()).await;
    let mut changed = Script::stock();
    let main = format!(
        r#"{},{{"dst":"203.0.113.0/24","gateway":"192.0.2.99","dev":"dummy0","flags":[]}}]"#,
        fixture("routes_main_v4").trim().trim_end_matches(']')
    );
    changed
        .tables
        .insert(254, (main, fixture("routes_main_v6")));
    assert_eq!(
        hash_of(&changed).await,
        before,
        "a route that is neither a default route nor an endpoint's route is not an input"
    );
}

#[tokio::test]
async fn route_replacement_and_deletion_detected() {
    let before = hash_of(&Script::stock()).await;

    let mut replaced = Script::stock();
    let main = fixture("routes_main_v4").replace("192.0.2.1", "192.0.2.254");
    replaced
        .tables
        .insert(254, (main, fixture("routes_main_v6")));
    for json in replaced.lookups.values_mut() {
        *json = json.replace("\"gateway\":\"192.0.2.1\"", "\"gateway\":\"192.0.2.254\"");
    }
    assert_ne!(hash_of(&replaced).await, before, "a replaced default route");

    let mut via = Script::stock();
    via_table_100(&mut via);
    let routed = hash_of(&via).await;
    // The table 100 route is deleted: the kernel then answers from the main table again.
    let mut deleted = via.clone();
    deleted.lookups.insert(
        "198.51.100.0".to_owned(),
        Script::stock().lookups["198.51.100.0"].clone(),
    );
    deleted.tables.insert(
        100,
        (
            fixture("routes_table100_v4").replace(
                r#",{"dst":"198.51.100.0/24","gateway":"192.0.2.2","dev":"dummy0","flags":[]}"#,
                "",
            ),
            fixture("routes_table100_v6"),
        ),
    );
    assert_ne!(hash_of(&deleted).await, routed, "a deleted route");
}

#[tokio::test]
async fn multiple_tables_and_rules_observed() {
    let mut script = Script::stock();
    via_table_100(&mut script);
    let lines = observe_script(&script).await.expect("observed");
    assert_eq!(lines.rule.len(), 7, "five IPv4 rules and two IPv6 rules");
    assert!(lines.rule.iter().any(|r| r.contains("fwmark")));
    assert!(
        lines
            .route
            .contains(&"inet|100|0.0.0.0/0|192.0.2.254|dummy0|200".to_owned())
    );
    assert!(
        lines
            .route
            .contains(&"inet|100|198.51.100.0/24|192.0.2.2|dummy0|0".to_owned())
    );
    assert!(
        lines
            .route
            .contains(&"inet|254|0.0.0.0/0|192.0.2.1|dummy0|0".to_owned())
    );
}

#[tokio::test]
async fn multiple_interfaces_and_gateways_observed() {
    let mut script = Script::stock();
    script.tables.insert(
        254,
        (
            r#"[{"dst":"default","gateway":"192.0.2.1","dev":"dummy0","flags":[]},{"dst":"192.0.2.0/24","dev":"dummy0","flags":[]},{"dst":"198.51.100.0/24","gateway":"192.0.2.50","dev":"eth1","flags":[]}]"#.to_owned(),
            "[]".to_owned(),
        ),
    );
    script.lookups.insert(
        "198.51.100.0".to_owned(),
        r#"[{"type":"1","dst":"198.51.100.0","gateway":"192.0.2.50","dev":"eth1","table":"254","prefsrc":"192.0.2.60","flags":[],"uid":0,"cache":[]}]"#.to_owned(),
    );
    script.addresses = r#"[{"ifindex":2,"ifname":"dummy0","addr_info":[{"family":"inet","local":"192.0.2.10","prefixlen":24}]},{"ifindex":3,"ifname":"eth1","addr_info":[{"family":"inet","local":"192.0.2.60","prefixlen":24}]},{"ifindex":4,"ifname":"unrelated0","addr_info":[{"family":"inet","local":"203.0.113.1","prefixlen":24}]}]"#.to_owned();
    let lines = observe_script(&script).await.expect("observed");
    assert!(
        lines
            .local_addr
            .contains(&"192.0.2.10/24|dummy0".to_owned())
    );
    assert!(lines.local_addr.contains(&"192.0.2.60/24|eth1".to_owned()));
    assert_eq!(
        lines.local_addr.len(),
        2,
        "only interfaces an endpoint route names"
    );
    assert!(
        lines
            .route
            .contains(&"inet|254|198.51.100.0/24|192.0.2.50|eth1|0".to_owned())
    );
}

#[tokio::test]
async fn vpn_tunnel_route_to_hub_observed() {
    let mut script = Script::stock();
    script.tables.insert(
        254,
        (
            r#"[{"dst":"default","dev":"tun0","scope":"253","flags":[]}]"#.to_owned(),
            "[]".to_owned(),
        ),
    );
    script.lookups.clear();
    script.lookups.insert(
        "192.0.2.77".to_owned(),
        r#"[{"type":"1","dst":"192.0.2.77","dev":"tun0","table":"254","prefsrc":"198.51.100.9","flags":[],"uid":0,"cache":[]}]"#.to_owned(),
    );
    script.lookups.insert(
        "198.51.100.0".to_owned(),
        r#"[{"type":"1","dst":"198.51.100.0","dev":"tun0","table":"254","prefsrc":"198.51.100.9","flags":[],"uid":0,"cache":[]}]"#.to_owned(),
    );
    script.addresses = r#"[{"ifindex":5,"ifname":"tun0","addr_info":[{"family":"inet","local":"198.51.100.9","prefixlen":32}]}]"#.to_owned();
    let lines = observe_script(&script).await.expect("observed");
    assert_eq!(
        lines.endpoint_route,
        [
            "192.0.2.77|254|-|tun0|198.51.100.9",
            "198.51.100.0|254|-|tun0|198.51.100.9"
        ]
    );
    assert_eq!(lines.local_addr, ["198.51.100.9/32|tun0"]);
    assert!(
        lines
            .route
            .contains(&"inet|254|0.0.0.0/0|-|tun0|0".to_owned())
    );
}

fn token(result: Result<domovoi_core::attest::RouteLines, ReadError>) -> &'static str {
    result.expect_err("fails closed").token()
}

#[tokio::test]
async fn incomplete_route_data_fails_closed() {
    // No `ip` program: the first read fails.
    let none = Arc::new(FakeProcessExecutor::new());
    assert_eq!(
        token(observe_with(none, "192.0.2.77", &[]).await),
        "rules_unreadable"
    );

    // Output that is not complete JSON is a partial read.
    let mut script = Script::stock();
    script.rules6 = "[{\"priority\":0,".to_owned();
    assert_eq!(token(observe_script(&script).await), "route_read_partial");

    // A table that fails to read names itself.
    let exec = Arc::new(FakeProcessExecutor::new());
    Script::stock().apply(&exec);
    exec.expect(
        IP,
        &["-N", "-j", "-4", "route", "show", "table", "254"],
        ProcessOutput {
            exit_code: Some(1),
            ..ProcessOutput::default()
        },
    );
    assert_eq!(
        token(observe_with(exec, "192.0.2.77", &["198.51.100.0/24"]).await),
        "table_unreadable"
    );

    // A lookup that fails.
    let exec = Arc::new(FakeProcessExecutor::new());
    Script::stock().apply(&exec);
    exec.expect(
        IP,
        &["-N", "-j", "-d", "-4", "route", "get", "192.0.2.77"],
        ProcessOutput {
            exit_code: Some(2),
            stderr: "RTNETLINK answers: Network is unreachable\n".to_owned(),
            ..ProcessOutput::default()
        },
    );
    assert_eq!(
        token(observe_with(exec, "192.0.2.77", &["198.51.100.0/24"]).await),
        "lookup_failed"
    );

    // The address read fails.
    let exec = Arc::new(FakeProcessExecutor::new());
    Script::stock().apply(&exec);
    exec.expect(
        IP,
        &["-N", "-j", "addr", "show"],
        ProcessOutput {
            exit_code: Some(1),
            ..ProcessOutput::default()
        },
    );
    assert_eq!(
        token(observe_with(exec, "192.0.2.77", &["198.51.100.0/24"]).await),
        "addresses_unreadable"
    );

    // The route the kernel selected is not in the table dump.
    let mut script = Script::stock();
    script
        .tables
        .insert(254, ("[]".to_owned(), "[]".to_owned()));
    assert_eq!(token(observe_script(&script).await), "route_read_partial");
}

#[tokio::test]
async fn truncated_ip_output_is_a_failed_read() {
    for (cut, expected) in [
        (
            ProcessOutput {
                exit_code: Some(0),
                stdout: fixture("rules_v4_default"),
                truncated: true,
                ..ProcessOutput::default()
            },
            "rules_unreadable",
        ),
        (
            ProcessOutput {
                timed_out: true,
                ..ProcessOutput::default()
            },
            "rules_unreadable",
        ),
        (
            ProcessOutput {
                signal: Some(9),
                ..ProcessOutput::default()
            },
            "rules_unreadable",
        ),
    ] {
        let exec = Arc::new(FakeProcessExecutor::new());
        Script::stock().apply(&exec);
        exec.expect(IP, &["-N", "-j", "-4", "rule", "show"], cut);
        assert_eq!(token(observe_with(exec, "192.0.2.77", &[]).await), expected);
    }
}

#[tokio::test]
async fn blackhole_rule_or_route_is_incomplete_not_hashed() {
    let mut rule = Script::stock();
    rule.rules4 = fixture("rules_v4_blackhole");
    assert_eq!(token(observe_script(&rule).await), "route_unrepresentable");

    for name in ["rules_v4_goto", "rules_v4_suppress"] {
        let mut script = Script::stock();
        script.rules4 = fixture(name);
        assert_eq!(
            token(observe_script(&script).await),
            "route_unrepresentable",
            "{name}"
        );
    }

    let referencing = r#"[{"priority":0,"src":"all","table":"255"},{"priority":50,"src":"all","table":"103"},{"priority":32766,"src":"all","table":"254"}]"#;
    let mut route = Script::stock();
    route.rules4 = referencing.to_owned();
    route
        .tables
        .insert(103, (fixture("routes_blackhole_default"), "[]".to_owned()));
    assert_eq!(token(observe_script(&route).await), "route_unrepresentable");

    let mut unreachable = Script::stock();
    unreachable.rules6 = referencing.to_owned();
    unreachable.tables.insert(
        103,
        ("[]".to_owned(), fixture("routes_unreachable_default_v6")),
    );
    assert_eq!(
        token(observe_script(&unreachable).await),
        "route_unrepresentable"
    );
}

#[tokio::test]
async fn a_lookup_that_selects_a_local_route_is_unrepresentable() {
    let mut script = Script::stock();
    script
        .lookups
        .insert("192.0.2.77".to_owned(), fixture("get_local_v4"));
    assert_eq!(
        token(observe_script(&script).await),
        "route_unrepresentable"
    );
}

#[tokio::test]
async fn multipath_route_is_route_unrepresentable() {
    let mut script = Script::stock();
    script.rules4 = r#"[{"priority":0,"src":"all","table":"255"},{"priority":50,"src":"all","table":"102"},{"priority":32766,"src":"all","table":"254"}]"#.to_owned();
    script
        .tables
        .insert(102, (fixture("routes_multipath"), "[]".to_owned()));
    assert_eq!(
        token(observe_script(&script).await),
        "route_unrepresentable"
    );
}

#[tokio::test]
async fn non_utf8_interface_name_is_route_unrepresentable() {
    let mut script = Script::stock();
    script.tables.insert(
        254,
        (
            "[{\"dst\":\"default\",\"gateway\":\"192.0.2.1\",\"dev\":\"tun\u{fffd}\"}]".to_owned(),
            "[]".to_owned(),
        ),
    );
    assert_eq!(
        token(observe_script(&script).await),
        "route_unrepresentable"
    );
}

#[tokio::test]
async fn a_table_that_does_not_exist_has_no_routes() {
    // Table 253 is referenced by every stock host's last rule and does not exist: the kernel
    // answers `[]`, an error text and exit 2. That is an empty table, not a failed read.
    let lines = observe_script(&Script::stock()).await.expect("observed");
    assert!(lines.rule.iter().any(|r| r.ends_with("|253")));
    assert!(lines.route.iter().all(|r| !r.contains("|253|")));
}

#[tokio::test]
async fn ip_is_run_with_argv_and_a_fixed_environment() {
    let exec = Arc::new(FakeProcessExecutor::new());
    Script::stock().apply(&exec);
    observe_with(exec.clone(), "192.0.2.77", &["198.51.100.0/24"])
        .await
        .expect("observed");
    let calls = exec.calls();
    assert!(!calls.is_empty());
    for call in &calls {
        assert_eq!(call.program, IP);
        assert_eq!(
            call.env.iter().collect::<Vec<_>>(),
            [(&"LC_ALL".to_owned(), &"C".to_owned())],
            "the whole environment is LC_ALL=C"
        );
        assert_eq!(call.timeout.as_secs(), 5);
        assert_eq!(call.max_output_bytes, 4 * 1024 * 1024);
        assert!(call.stdin.is_none() && call.run_as.is_none() && call.cwd.is_none());
        assert!(call.args.starts_with(&["-N".to_owned(), "-j".to_owned()]));
        for arg in &call.args {
            assert!(
                arg.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b':'),
                "{arg} is a plain word, an address or a number"
            );
        }
    }
    let argv: Vec<String> = calls.iter().map(|c| c.args.join(" ")).collect();
    for expected in [
        "-N -j -4 rule show",
        "-N -j -6 rule show",
        "-N -j -4 route show table 254",
        "-N -j -6 route show table 255",
        "-N -j addr show",
        "-N -j -d -4 route get 192.0.2.77",
        "-N -j -d -4 route get 198.51.100.0",
    ] {
        assert!(
            argv.iter().any(|a| a == expected),
            "missing {expected}: {argv:?}"
        );
    }
}

#[tokio::test]
async fn a_capture_from_a_stock_host_observes_end_to_end() {
    // Real `ip -N -j` output of an ordinary host with one interface and a default gateway
    // (addresses scrubbed to documentation ranges).
    let mut script = Script::stock();
    script.rules4 = fixture("stock_rules_v4");
    script.rules6 = fixture("stock_rules_v6");
    script.tables.clear();
    script.tables.insert(
        254,
        (
            fixture("stock_routes_main_v4"),
            fixture("stock_routes_main_v6"),
        ),
    );
    script
        .tables
        .insert(255, (fixture("stock_routes_local_v4"), "[]".to_owned()));
    script.addresses = fixture("stock_addresses");
    script.lookups.clear();
    for endpoint in ["192.0.2.77", "198.51.100.0"] {
        script.lookups.insert(
            endpoint.to_owned(),
            fixture("stock_get_v4").replace("1.1.1.1", endpoint),
        );
    }
    let lines = observe_script(&script).await.expect("observed");
    assert_eq!(
        lines.endpoint_route,
        [
            "192.0.2.77|254|198.51.100.1|eth0|198.51.100.5",
            "198.51.100.0|254|198.51.100.1|eth0|198.51.100.5"
        ]
    );
    assert!(
        lines
            .route
            .contains(&"inet|254|0.0.0.0/0|198.51.100.1|eth0|0".to_owned())
    );
    assert_eq!(
        lines.local_addr,
        ["198.51.100.5/24|eth0", "fe80::200:5eff:fe00:5301/64|eth0"],
        "every address of the interface an endpoint route names"
    );
    assert_eq!(lines.rule.len(), 5);
}
