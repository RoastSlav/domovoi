//! Property tests: the parsers accept only what they render, for any input.

use core::net::IpAddr;

use domovoi_core::attest::{
    BootId, Cidr, ExpectedSafety, Family, Hash32, Report, Route, Rule, Selector, UnverifiedReason,
    decode_field, encode_field, parse_ip, parse_report, render_complete, render_incomplete,
};
use proptest::prelude::*;

use crate::common::{complete_report, incomplete_report};

fn render(report: &Report) -> Vec<u8> {
    match report {
        Report::Complete(complete) => render_complete(complete),
        Report::Incomplete(incomplete) => render_incomplete(incomplete),
    }
}

fn ip_strategy() -> impl Strategy<Value = IpAddr> {
    prop_oneof![
        any::<[u8; 4]>().prop_map(IpAddr::from),
        any::<[u16; 8]>().prop_map(IpAddr::from),
        // Many zero groups, to reach every compression shape.
        prop::array::uniform8(prop_oneof![Just(0_u16), Just(1_u16), any::<u16>()])
            .prop_map(IpAddr::from),
    ]
}

fn name() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..=15)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(160))]

    #[test]
    fn parse_report_accepts_only_what_it_renders(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        if let Ok(report) = parse_report(&bytes) {
            prop_assert_eq!(render(&report), bytes);
        }
    }

    #[test]
    fn single_byte_mutation_of_a_valid_report_is_rejected_or_rerenders_identically(
        index in any::<prop::sample::Index>(),
        byte in any::<u8>(),
        complete in any::<bool>(),
    ) {
        let valid = if complete {
            render_complete(&complete_report())
        } else {
            render_incomplete(&incomplete_report())
        };
        let mut mutated = valid.clone();
        let at = index.index(mutated.len());
        mutated[at] = byte;
        if let Ok(report) = parse_report(&mutated) {
            prop_assert_eq!(render(&report), mutated);
        }
    }

    #[test]
    fn field_codec_round_trips_every_byte_string(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
        let encoded = encode_field(&bytes);
        prop_assert_eq!(decode_field(&encoded).expect("our own output decodes"), bytes);
        prop_assert!(encoded.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:/@-%0123456789ABCDEF".contains(&b)));
    }

    #[test]
    fn encode_of_decode_is_identity_for_accepted_text(text in "[A-Za-z0-9._:/@%|=; -]{0,24}") {
        if let Ok(bytes) = decode_field(&text) {
            prop_assert_eq!(encode_field(&bytes), text);
        }
    }

    #[test]
    fn selector_parse_render_round_trip_for_arbitrary_interface_names(
        iif in prop::option::of(name()),
        oif in prop::option::of(name()),
        not in any::<bool>(),
        mark in prop::option::of((any::<u32>(), any::<u32>())),
        proto in prop::option::of(any::<u8>()),
        ports in prop::option::of((any::<u16>(), any::<u16>())),
    ) {
        let selector = Selector {
            iif,
            oif,
            not,
            fwmark: mark,
            ipproto: proto,
            sport: ports.map(|(a, b)| (a.min(b), a.max(b))),
            ..Selector::default()
        };
        let text = selector.render();
        prop_assert_eq!(&Selector::parse(&text).expect("our own output parses"), &selector);
        let rule = Rule { family: Family::Inet, priority: 7, selector: selector.clone(), table: 9 };
        let line = rule.render();
        prop_assert_eq!(line.matches('|').count(), 3);
        prop_assert_eq!(Rule::parse(&line).expect("our own line parses"), rule);
    }

    #[test]
    fn ip_text_is_canonical_exactly_when_it_matches_the_formatter(ip in ip_strategy()) {
        let text = ip.to_string();
        prop_assert_eq!(parse_ip(&text).expect("canonical"), ip);
        if ip.is_ipv6() {
            prop_assert!(parse_ip(&text.to_uppercase()).is_err() || text == text.to_uppercase());
        }
    }

    #[test]
    fn route_line_round_trips(
        table in any::<u32>(),
        metric in any::<u32>(),
        prefix in 0_u8..=32,
        addr in any::<[u8; 4]>(),
        gateway in prop::option::of(any::<[u8; 4]>()),
        dev in name(),
    ) {
        let dst = Cidr::new(IpAddr::from(addr), prefix).expect("a prefix").network();
        let route = Route {
            family: Family::Inet,
            table,
            dst,
            gateway: gateway.map(IpAddr::from),
            ifname: dev,
            metric,
        };
        let line = route.render();
        prop_assert_eq!(Route::parse(&line).expect("our own line parses"), route);
    }

    #[test]
    fn expected_safety_round_trips(
        fingerprint in any::<[u8; 32]>(),
        config in any::<[u8; 32]>(),
        routes in any::<[u8; 32]>(),
        generation in any::<u64>(),
        verified in any::<bool>(),
        reason in prop::sample::select(UnverifiedReason::ALL.to_vec()),
    ) {
        let tag = if verified {
            ExpectedSafety::Verified {
                fingerprint: Hash32::from_bytes(fingerprint),
                boot_id: BootId::parse("5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f").expect("boot"),
                generation,
                config_hash: Hash32::from_bytes(config),
                route_state_hash: Hash32::from_bytes(routes),
            }
        } else {
            ExpectedSafety::Unverified { reason }
        };
        let json = serde_json::to_string(&tag).expect("serializable");
        prop_assert_eq!(ExpectedSafety::parse_json(&json).expect("re-read"), tag);
    }
}
