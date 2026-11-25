//! Tests of the pure containment core: networks, exclusions, tokens, gate and journal.

use jiff::Timestamp;

use super::*;

fn net(text: &str) -> Net {
    Net::parse(text).unwrap()
}

fn policy() -> TargetPolicy {
    TargetPolicy {
        min_prefix_v4: 24,
        min_prefix_v6: 64,
    }
}

fn inputs(management: &[&str]) -> ExclusionInputs {
    ExclusionInputs {
        management_cidrs: management.iter().map(|text| net(text)).collect(),
        ..ExclusionInputs::default()
    }
}

fn at() -> Timestamp {
    Timestamp::constant(1_700_000_000, 0)
}

#[test]
fn canonical_ipv4_and_ipv6_text_is_stable() {
    assert_eq!(net("203.0.113.7").canonical(), "203.0.113.7/32");
    assert_eq!(net("203.0.113.0/24").canonical(), "203.0.113.0/24");
    assert_eq!(net("2001:db8::").canonical(), "2001:db8::/128");
    assert_eq!(net("2001:DB8:0:0::/64").canonical(), "2001:db8::/64");
    assert_eq!(net("203.0.113.7").element_text(), "203.0.113.7");
    assert_eq!(net("203.0.113.0/24").element_text(), "203.0.113.0/24");
    for text in ["203.0.113.0/24", "2001:db8::/64", "198.51.100.9/32"] {
        assert_eq!(
            net(text).canonical(),
            net(&net(text).canonical()).canonical()
        );
    }
}

#[test]
fn mapped_v6_target_refused_and_mapped_exclusion_canonicalised() {
    assert_eq!(Net::parse("::ffff:192.0.2.1"), Err(NetError::MappedForm));
    assert_eq!(Net::parse("::ffff:0:0/96"), Err(NetError::MappedForm));
    assert_eq!(Net::parse("::/96"), Err(NetError::MappedForm));
    let mapped = Net::parse_exclusion("::ffff:192.0.2.0/120").unwrap();
    assert_eq!(mapped.canonical(), "192.0.2.0/24");
    let all = effective_exclusions(Some(&ExclusionInputs {
        hub_addrs: vec![Net::parse_exclusion("::ffff:192.0.2.1").unwrap()],
        ..inputs(&["10.0.0.0/8"])
    }))
    .unwrap();
    assert!(all.iter().any(|n| n.canonical() == "192.0.2.1/32"));
    let relaxed = TargetPolicy {
        min_prefix_v4: 1,
        min_prefix_v6: 1,
    };
    assert_eq!(
        check_target(&net("192.0.2.1"), &all, &relaxed),
        Err(TargetRefusal::ExclusionOverlap(net("192.0.2.1")))
    );
}

#[test]
fn hostname_and_prefix_with_host_bits_refused() {
    assert_eq!(Net::parse("example.com"), Err(NetError::Invalid));
    assert_eq!(Net::parse("203.0.113.1/24"), Err(NetError::HostBits));
    assert_eq!(Net::parse("203.0.113.0/33"), Err(NetError::PrefixLength));
    assert_eq!(Net::parse("203.0.113.0/"), Err(NetError::PrefixLength));
    assert_eq!(Net::parse("203.0.113.0/024"), Err(NetError::PrefixLength));
    assert_eq!(Net::parse("203.000.113.0"), Err(NetError::Invalid));
    assert_eq!(Net::parse("2001:db8::/129"), Err(NetError::PrefixLength));
    assert_eq!(Net::parse(""), Err(NetError::Invalid));
}

#[test]
fn excluded_target_refused_for_overlap_in_both_directions_v4_and_v6() {
    let ex = effective_exclusions(Some(&inputs(&["192.0.2.0/24", "2001:db8:1::/48"]))).unwrap();
    let relaxed = TargetPolicy {
        min_prefix_v4: 8,
        min_prefix_v6: 32,
    };
    for text in [
        "192.0.2.9",
        "192.0.0.0/16",
        "192.0.2.0/24",
        "2001:db8:1::5",
        "2001:db8::/32",
    ] {
        assert!(
            matches!(
                check_target(&net(text), &ex, &relaxed),
                Err(TargetRefusal::ExclusionOverlap(_))
            ),
            "{text}"
        );
    }
    assert_eq!(
        check_target(&net("198.51.100.0/24"), &ex, &policy()),
        Ok(())
    );
    assert_eq!(
        check_target(&net("2001:db8:2::/64"), &ex, &policy()),
        Ok(())
    );
    // An IPv4 target is never compared with an IPv6 exclusion.
    assert!(!net("192.0.2.1").overlaps(&net("2001:db8::/32")));
}

#[test]
fn prefix_floor_is_checked_first_and_never_allows_slash_zero() {
    let ex = effective_exclusions(Some(&inputs(&["192.0.2.0/24"]))).unwrap();
    assert_eq!(
        check_target(&net("198.51.0.0/16"), &ex, &policy()),
        Err(TargetRefusal::PrefixTooWide)
    );
    assert_eq!(
        check_target(&net("192.0.0.0/16"), &ex, &policy()),
        Err(TargetRefusal::PrefixTooWide)
    );
    let zero = TargetPolicy {
        min_prefix_v4: 0,
        min_prefix_v6: 0,
    };
    assert_eq!(
        check_target(&net("0.0.0.0/0"), &ex, &zero),
        Err(TargetRefusal::PrefixTooWide)
    );
}

#[test]
fn fixed_exclusions_cover_loopback_link_local_multicast() {
    let fixed = fixed_exclusions();
    for text in [
        "127.0.0.1",
        "169.254.1.1",
        "0.1.2.3",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "::",
        "fe80::1",
        "ff02::1",
    ] {
        let target = Net::parse_exclusion(text).unwrap();
        assert!(fixed.iter().any(|ex| ex.overlaps(&target)), "{text}");
    }
    assert!(!fixed.iter().any(|ex| ex.overlaps(&net("203.0.113.1"))));
    assert_eq!(fixed.len(), 9);
}

#[test]
fn incomplete_exclusion_inputs_refuse_all_containment() {
    assert_eq!(
        effective_exclusions(None),
        Err(ExclusionRefusal::Incomplete)
    );
}

#[test]
fn no_management_cidrs_refuses_containment() {
    assert_eq!(
        effective_exclusions(Some(&ExclusionInputs::default())),
        Err(ExclusionRefusal::NoManagementCidrs)
    );
}

#[test]
fn coalesce_merges_overlapping_and_adjacent_ranges() {
    let v6 = net("2001:db8::/64");
    let merged = coalesce(&[
        net("192.0.2.128/25"),
        net("192.0.2.0/25"),
        net("192.0.2.64/26"),
        net("198.51.100.7"),
        v6,
    ]);
    assert_eq!(
        merged,
        vec![
            (0xc000_0200, 0xc000_02ff, Family::V4),
            (0xc633_6407, 0xc633_6407, Family::V4),
            (v6.lo(), v6.hi(), Family::V6),
        ]
    );
}

#[test]
fn net_serde_round_trips_through_canonical_text() {
    let json = serde_json::to_string(&net("203.0.113.0/24")).unwrap();
    assert_eq!(json, "\"203.0.113.0/24\"");
    assert_eq!(
        serde_json::from_str::<Net>(&json).unwrap(),
        net("203.0.113.0/24")
    );
    assert!(serde_json::from_str::<Net>("\"::ffff:192.0.2.1\"").is_err());
}

#[test]
fn token_spellings_are_unique_and_round_trip() {
    let mut seen = std::collections::BTreeSet::new();
    for token in Token::ALL {
        assert!(seen.insert(token.as_str()));
        assert_eq!(Token::parse(token.as_str()), Some(token));
    }
    for rejected in [
        "overlaps_exclusion",
        "bad_target",
        "ttl_above_max_ttl",
        "table_missing",
        "absent_unexplained",
        "unverified_requires_human",
    ] {
        assert_eq!(Token::parse(rejected), None, "{rejected}");
    }
    let message = format_message(Token::ExclusionOverlap, "192.0.2.0/24");
    assert_eq!(message, "exclusion_overlap: 192.0.2.0/24");
    assert_eq!(
        parse_message(&message),
        Some((Token::ExclusionOverlap, "192.0.2.0/24"))
    );
    assert_eq!(parse_message("nonsense: x"), None);
}

#[test]
fn error_token_mapping_matches_recorded_stderr() {
    use NftOp::*;
    use StderrClass::*;
    let cases = [
        (
            Rollback,
            "Error: Could not process rule: Too many open files in system",
            SetFull,
        ),
        (
            Rollback,
            "Error: Could not process rule: No such file or directory",
            ElementAbsent,
        ),
        (
            Rollback,
            "Error: Could not process rule: element does not exist",
            ElementAbsent,
        ),
        (Rollback, "Error: interval not found in set", ElementAbsent),
        (
            Rollback,
            "Error: No such file or directory",
            TableOrSetMissing,
        ),
        (
            Confirm,
            "Error: No such file or directory",
            TableOrSetMissing,
        ),
        (
            Rollback,
            "Error: interval overlaps with an existing one",
            IntervalOverlap,
        ),
        (
            CreateElement,
            "Error: Could not process rule: File exists",
            AlreadyPresent,
        ),
        (
            CreateTable,
            "Error: Could not process rule: File exists",
            TableExistsRace,
        ),
        (
            Confirm,
            "Error: Could not process rule: Invalid argument",
            TableDrift,
        ),
        (
            CreateElement,
            "Error: You must add 'flags interval' to your set",
            TableDrift,
        ),
        (
            Confirm,
            "Error: syntax error, unexpected timeout",
            InternalTemplateError,
        ),
        (Confirm, "Error: value too large", InternalTemplateError),
        (Confirm, "Error: wrong time format", InternalTemplateError),
        (
            Bootstrap,
            "Error: \"destroy\" command is not supported",
            InternalTemplateError,
        ),
        (
            CreateElement,
            "Error: Prefix length 40 is invalid for IPv4",
            InvalidTarget,
        ),
        (
            CreateElement,
            "Error: Could not resolve hostname",
            InvalidTarget,
        ),
        (
            CreateElement,
            "Error: Could not process rule: Cannot allocate memory",
            ResourceExhausted,
        ),
        (
            Bootstrap,
            "Error: Chain of type \"filter\" is not supported, perhaps kernel support is missing?",
            ChainUnsupported,
        ),
        (Read, "Error: Operation not permitted", PermissionDenied),
        (Read, "something else", Unknown),
    ];
    for (op, text, class) in cases {
        assert_eq!(classify_stderr(op, text), class, "{text}");
    }
    assert_eq!(
        IntervalOverlap.token(true, true),
        Some(Token::CoveredByOther)
    );
    assert_eq!(IntervalOverlap.token(false, true), Some(Token::TableDrift));
    assert_eq!(ChainUnsupported.token(false, true), Some(Token::TableDrift));
    assert_eq!(
        ChainUnsupported.token(false, false),
        Some(Token::UnsupportedKernel)
    );
    assert_eq!(ElementAbsent.token(true, true), None);
    assert_eq!(TableOrSetMissing.token(true, true), None);
}

#[test]
fn bare_no_such_file_is_table_missing_not_expired() {
    let facts = AbsentFacts {
        table_exists: false,
        exact_member: false,
        covering_member: false,
        rollback_recorded: false,
        past_step1_deadline: true,
    };
    assert_eq!(classify_absent(&facts), AbsentClass::TableMissing);
    assert_eq!(classify_absent(&facts).token(), Token::ContainmentLost);
}

#[test]
fn absent_elements_are_told_apart_by_facts_not_text() {
    let base = AbsentFacts {
        table_exists: true,
        exact_member: false,
        covering_member: false,
        rollback_recorded: false,
        past_step1_deadline: false,
    };
    let with = |facts: AbsentFacts| classify_absent(&facts);
    assert_eq!(
        with(AbsentFacts {
            exact_member: true,
            ..base
        }),
        AbsentClass::PresentUnexpectedly
    );
    assert_eq!(
        with(AbsentFacts {
            covering_member: true,
            ..base
        }),
        AbsentClass::CoveredByOther
    );
    assert_eq!(
        with(AbsentFacts {
            rollback_recorded: true,
            past_step1_deadline: true,
            ..base
        }),
        AbsentClass::RolledBack
    );
    assert_eq!(
        with(AbsentFacts {
            past_step1_deadline: true,
            ..base
        }),
        AbsentClass::Expired
    );
    assert_eq!(with(base), AbsentClass::AbsentUnexplained);
    assert_eq!(
        AbsentClass::AbsentUnexplained.token(),
        Token::ContainmentLost
    );
}

fn key(layout: Layout) -> SelftestKey {
    SelftestKey {
        kernel_release: "6.8.0".to_owned(),
        nft_version: "nftables v1.0.9".to_owned(),
        layout,
    }
}

fn result(hosts: Verdict, prefixes: Option<Verdict>, layout: Layout) -> SelftestResult {
    SelftestResult {
        key: key(layout),
        hosts,
        prefixes,
        ran_at: at(),
        replacements: 20_000,
        control_leak_ratio: 0.4,
    }
}

fn inconclusive() -> Verdict {
    Verdict::Inconclusive {
        reason: "replacements_not_reached".to_owned(),
    }
}

#[test]
fn gate_mapping_matches_the_table_of_6_6() {
    let both = Layout::HostsAndPrefixes;
    let current = key(both);
    let gate = |r: &SelftestResult| gate_for(Some(r), &current, true);
    assert_eq!(
        gate(&result(Verdict::Pass, Some(Verdict::Pass), both)),
        Gate::Open {
            interval_path: true
        }
    );
    assert_eq!(
        gate(&result(
            Verdict::Pass,
            Some(Verdict::Fail { leaked: 3 }),
            both
        )),
        Gate::Open {
            interval_path: false
        }
    );
    assert_eq!(
        gate(&result(Verdict::Pass, None, both)),
        Gate::Open {
            interval_path: false
        }
    );
    assert_eq!(
        gate(&result(
            Verdict::Fail { leaked: 1 },
            Some(Verdict::Pass),
            both
        )),
        Gate::BlockedLeak
    );
    let untested = |reason| Gate::BlockedUntested { reason };
    assert_eq!(
        gate(&result(inconclusive(), None, both)),
        untested(UntestedReason::Inconclusive)
    );
    assert_eq!(
        gate(&result(Verdict::Pass, Some(inconclusive()), both)),
        untested(UntestedReason::Inconclusive)
    );
    assert_eq!(
        gate_for(None, &current, true),
        untested(UntestedReason::NeverRun)
    );
}

#[test]
fn selftest_result_is_keyed_by_kernel_nft_and_layout() {
    let stored = result(Verdict::Pass, None, Layout::Hosts);
    let same = key(Layout::Hosts);
    assert_eq!(
        gate_for(Some(&stored), &same, true),
        Gate::Open {
            interval_path: false
        }
    );
    let mut kernel = same.clone();
    kernel.kernel_release = "6.9.0".to_owned();
    let mut nft = same.clone();
    nft.nft_version = "nftables v1.1.0".to_owned();
    let layout = key(Layout::HostsAndPrefixes);
    for changed in [kernel, nft, layout] {
        assert_eq!(
            gate_for(Some(&stored), &changed, true),
            Gate::BlockedUntested {
                reason: UntestedReason::KeyChanged
            }
        );
    }
}

#[test]
fn untested_platform_without_opt_in_blocks_even_a_passing_result() {
    let stored = result(Verdict::Pass, Some(Verdict::Pass), Layout::HostsAndPrefixes);
    assert_eq!(
        gate_for(Some(&stored), &key(Layout::HostsAndPrefixes), false),
        Gate::BlockedUntested {
            reason: UntestedReason::PlatformUntested
        }
    );
}

#[test]
fn leak_self_test_inconclusive_blocks_containment() {
    let stored = result(inconclusive(), None, Layout::Hosts);
    let gate = gate_for(Some(&stored), &key(Layout::Hosts), true);
    assert!(!matches!(gate, Gate::Open { .. }));
}

const BLOCK: &str = "018f4e46-4a2e-7a37-9d0a-4f2b6c8d0e1f";

fn record_lines() -> Vec<String> {
    [
        JournalRecord::Intent {
            op: JournalOp::Apply,
            block: BLOCK.to_owned(),
            source: "203.0.113.7/32".to_owned(),
            set: "blocked4".to_owned(),
            ttl_s: 600,
            at: at(),
        },
        JournalRecord::Applied {
            block: BLOCK.to_owned(),
            at: at(),
        },
        JournalRecord::Confirmed {
            block: BLOCK.to_owned(),
            deadline: at(),
        },
    ]
    .iter()
    .map(|record| record.to_line().unwrap())
    .collect()
}

#[test]
fn journal_records_round_trip_one_per_line() {
    let text = record_lines().join("\n") + "\n";
    let read = read_journal(&text).unwrap();
    assert_eq!(read.records.len(), 3);
    assert!(!read.torn_final_line);
    assert_eq!(read.records[0].block(), Some(BLOCK));
}

#[test]
fn torn_final_journal_line_is_ignored_and_corrupt_middle_refuses() {
    let lines = record_lines();
    let torn = format!("{}\n{}\n{{\"rec\":\"conf", lines[0], lines[1]);
    let read = read_journal(&torn).unwrap();
    assert_eq!(read.records.len(), 2);
    assert!(read.torn_final_line);
    let corrupt = format!("{}\n{{broken\n{}\n", lines[0], lines[1]);
    assert_eq!(read_journal(&corrupt), Err(JournalCorrupt { line: 2 }));
    let bad_last = format!("{}\n{{broken\n", lines[0]);
    assert_eq!(read_journal(&bad_last), Err(JournalCorrupt { line: 2 }));
}

#[test]
fn ip_text_follows_rfc_5952_and_rejects_malformed_input() {
    let text = |input: &str| Net::parse_exclusion(input).map(|n| n.canonical());
    assert_eq!(
        text("2001:db8:0:0:1:0:0:1/128").unwrap(),
        "2001:db8::1:0:0:1/128"
    );
    assert_eq!(text("1:0:2:3:4:5:6:7").unwrap(), "1:0:2:3:4:5:6:7/128");
    assert_eq!(text("::").unwrap(), "::/128");
    assert_eq!(text("1::").unwrap(), "1::/128");
    assert_eq!(text("::1").unwrap(), "::1/128");
    for bad in [
        ":::",
        "1::2::3",
        "12345::",
        "1:2:3:4:5:6:7:8:9",
        "1:2:3",
        "g::1",
        ":1::",
        "1.2.3",
        "256.1.1.1",
        "1.2.3.4.5",
        " 1.2.3.4",
    ] {
        assert_eq!(text(bad), Err(NetError::Invalid), "{bad}");
    }
}

#[test]
fn argument_keys_are_exactly_the_six() {
    assert_eq!(
        keys::ARG_KEYS,
        [
            "phase",
            "source",
            "ttl_s",
            "block",
            "prefix_len_v4",
            "prefix_len_v6"
        ]
    );
}
