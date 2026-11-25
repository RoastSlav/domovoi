//! Tests and snapshots of the nft script templates.

use super::templates::*;
use super::{Family, Net};

fn net(text: &str) -> Net {
    Net::parse(text).unwrap()
}

fn table() -> TableName {
    TableName::default()
}

fn max() -> MaxElements {
    MaxElements::new(1000).unwrap()
}

#[test]
fn templates_match_the_snapshots() {
    insta::assert_snapshot!("bootstrap_hosts", bootstrap(&table(), Layout::Hosts, max()));
    insta::assert_snapshot!(
        "bootstrap_hosts_and_prefixes",
        bootstrap(&table(), Layout::HostsAndPrefixes, max())
    );
    insta::assert_snapshot!("apply_host_v4", apply(&table(), &net("203.0.113.7")));
    insta::assert_snapshot!("apply_prefix_v4", apply(&table(), &net("203.0.113.0/24")));
    insta::assert_snapshot!("apply_host_v6", apply(&table(), &net("2001:db8::7")));
    insta::assert_snapshot!("apply_prefix_v6", apply(&table(), &net("2001:db8:1::/64")));
    let ttl = Seconds::new(3600).unwrap();
    insta::assert_snapshot!(
        "confirm_host",
        confirm(&table(), &net("203.0.113.7"), ttl).unwrap()
    );
    insta::assert_snapshot!(
        "confirm_prefix",
        confirm(&table(), &net("203.0.113.0/24"), ttl).unwrap()
    );
    insta::assert_snapshot!("rollback_host", rollback(&table(), &net("203.0.113.7")));
    insta::assert_snapshot!(
        "rollback_prefix",
        rollback(&table(), &net("203.0.113.0/24"))
    );
    insta::assert_snapshot!(
        "rollback_fallback",
        rollback_fallback(&table(), &net("203.0.113.7"))
    );
    let v4 = [(0xc000_0200, 0xc000_02ff), (0xc633_6407, 0xc633_6407)];
    insta::assert_snapshot!(
        "exclusion_rewrite_v4",
        exclusion_rewrite(&table(), Family::V4, &v4)
    );
    let v6net = net("2001:db8::/112");
    let v6 = [(v6net.lo(), v6net.hi())];
    insta::assert_snapshot!(
        "exclusion_rewrite_v6",
        exclusion_rewrite(&table(), Family::V6, &v6)
    );
    insta::assert_snapshot!(
        "exclusion_rewrite_empty",
        exclusion_rewrite(&table(), Family::V4, &[])
    );
    let reads = [
        list_table_args(&table()),
        list_set_args(&table(), SetName::Blocked4),
        list_flowtables_args(),
        script_args(),
        conntrack_delete_args(&net("203.0.113.7")),
        conntrack_delete_args(&net("203.0.113.0/24")),
    ];
    insta::assert_debug_snapshot!("reads", reads);
}

#[test]
fn ttl_zero_is_rejected_before_nft() {
    assert_eq!(Seconds::new(0), Err(TemplateError::ZeroSeconds));
}

#[test]
fn ttl_at_or_below_30_is_refused() {
    for secs in [1, 29, 30] {
        let ttl = Seconds::new(secs).unwrap();
        assert_eq!(
            confirm(&table(), &net("203.0.113.7"), ttl),
            Err(TemplateError::TtlTooShort)
        );
    }
    assert!(confirm(&table(), &net("203.0.113.7"), Seconds::new(31).unwrap()).is_ok());
}

#[test]
fn ttl_formatter_emits_only_positive_seconds() {
    let all = [
        bootstrap(&table(), Layout::HostsAndPrefixes, max()),
        apply(&table(), &net("203.0.113.7")),
        confirm(&table(), &net("203.0.113.7"), Seconds::new(86_400).unwrap()).unwrap(),
        rollback(&table(), &net("203.0.113.7")),
    ]
    .join("");
    for part in all.split("timeout ").skip(1) {
        let word: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let digits = word.strip_suffix('s').unwrap_or("");
        let n: u64 = digits.parse().unwrap();
        assert!(n >= 1, "{word}");
    }
    assert!(!all.contains("timeout 0"));
}

#[test]
fn max_elements_and_table_names_are_bounded() {
    assert!(MaxElements::new(0).is_err());
    assert!(MaxElements::new(10_001).is_err());
    assert!(MaxElements::new(10_000).is_ok());
    for bad in [
        "",
        "Domovoi",
        "1abc",
        "a-b",
        "a b",
        "domovoi;flush",
        &"a".repeat(33),
    ] {
        assert_eq!(
            TableName::parse(bad),
            Err(TemplateError::TableName),
            "{bad:?}"
        );
    }
    assert!(TableName::parse(&"a".repeat(32)).is_ok());
}

fn every_template() -> String {
    let n = net("203.0.113.0/24");
    let ttl = Seconds::new(600).unwrap();
    [
        bootstrap(&table(), Layout::Hosts, max()),
        bootstrap(&table(), Layout::HostsAndPrefixes, max()),
        apply(&table(), &n),
        confirm(&table(), &n, ttl).unwrap(),
        rollback(&table(), &n),
        rollback_fallback(&table(), &n),
        exclusion_rewrite(&table(), Family::V4, &[(1, 2)]),
        exclusion_rewrite(&table(), Family::V6, &[]),
    ]
    .join("")
}

#[test]
fn no_template_uses_add_for_step_one() {
    let step = apply(&table(), &net("203.0.113.7"));
    assert!(step.starts_with("create element "));
    assert!(!step.contains("add "));
}

#[test]
fn no_template_contains_destroy_flush_ruleset_or_a_foreign_table() {
    let all = every_template();
    assert!(!all.contains("destroy"));
    assert!(!all.contains("flush ruleset"));
    for line in all.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if let Some(at) = words.iter().position(|w| *w == "inet") {
            assert_eq!(words[at + 1], "domovoi", "{line}");
        }
    }
    let other = TableName::parse("other_table").unwrap();
    assert!(apply(&other, &net("203.0.113.7")).contains("inet other_table "));
}

#[test]
fn chain_below_minus_200_has_no_ct_rule() {
    const { assert!(CHAIN_PRIORITY < -200) };
    for layout in [Layout::Hosts, Layout::HostsAndPrefixes] {
        let text = bootstrap(&table(), layout, max());
        assert!(!text.split_whitespace().any(|w| w == "ct"));
        assert!(!text.contains("comment"));
    }
}

#[test]
fn chain_priority_is_not_a_known_manager_number() {
    for known in [-300, -200, -140, -100, -90, 0, 10, 100, 110] {
        assert_ne!(CHAIN_PRIORITY, known);
    }
    assert!(bootstrap(&table(), Layout::Hosts, max()).contains("priority -310;"));
}

#[test]
fn only_confirm_and_rollback_add_an_element_and_bootstrap_creates_the_table_once() {
    let boot = bootstrap(&table(), Layout::HostsAndPrefixes, max());
    assert!(boot.starts_with("create table inet domovoi\n"));
    assert_eq!(boot.matches("create table").count(), 1);
    for template in [
        apply(&table(), &net("203.0.113.7")),
        rollback_fallback(&table(), &net("203.0.113.7")),
    ] {
        assert!(!template.contains("add element"));
    }
    for template in [
        confirm(&table(), &net("203.0.113.7"), Seconds::new(60).unwrap()).unwrap(),
        rollback(&table(), &net("203.0.113.7")),
    ] {
        assert!(template.contains("add element"));
    }
}

#[test]
fn set_choice_follows_family_and_host_or_prefix() {
    assert_eq!(SetName::for_net(&net("203.0.113.7")), SetName::Blocked4);
    assert_eq!(SetName::for_net(&net("2001:db8::7")), SetName::Blocked6);
    assert_eq!(
        SetName::for_net(&net("203.0.113.0/24")),
        SetName::BlockedNet4
    );
    assert_eq!(
        SetName::for_net(&net("2001:db8::/64")),
        SetName::BlockedNet6
    );
}
