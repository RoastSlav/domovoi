//! The host ceiling against the real `firewall.block_source` contract: its two prefix floors are
//! the contract's constrainable arguments, and a request always carries both.

use domovoi_core::capability::{
    CeilingEntry, CeilingSummary, CeilingTargets, ContractIndex, known_contracts,
};
use domovoi_engine::policy::{CanonicalArgs, EvaluateRequest, HardLimit, Outcome::Allow};
use jiff::SignedDuration;

use crate::policy_fixtures::*;

const BLOCK: &str = "firewall.block_source";

fn ceiling_with(args: &[(&str, i64)], max_ttl: Option<SignedDuration>) -> CeilingSummary {
    CeilingSummary {
        enabled: true,
        entries: vec![CeilingEntry {
            contract: cid(BLOCK),
            targets: CeilingTargets::Any,
            max_per_hour: 10,
            max_active: Some(5),
            max_ttl,
            args: args.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
        }],
        hash: Some("h1".to_owned()),
    }
}

/// A block request built the way the hub builds one: both prefix lengths always present.
fn block(fx: &Fx, v4: u8, v6: u8, ttl_s: i64, ceiling: CeilingSummary) -> EvaluateRequest {
    let meta = ContractIndex::new(known_contracts())
        .unwrap()
        .meta(&cid(BLOCK))
        .unwrap();
    let mut request = fx.req(CONTAIN);
    request.contract = meta;
    request.args = CanonicalArgs::from_canonical(format!(
        "prefix_len_v4:{}:{v4}\nprefix_len_v6:{}:{v6}\n",
        v4.to_string().len(),
        v6.to_string().len()
    ));
    request.requested_ttl = Some(SignedDuration::from_secs(ttl_s));
    request.ceiling = Some(ceiling);
    request
}

#[test]
fn ceiling_covers_a_v6_target_under_a_v4_only_floor() {
    let fx = Fx::new();
    let rules = [rule("allow", Allow)];
    // A /64 IPv6 target: the request names its own length for v6 and the widest-safe 32 for v4,
    // so the v4-only floor of 24 is satisfied and the ceiling covers it.
    let request = block(
        &fx,
        32,
        64,
        600,
        ceiling_with(&[("prefix_len_v4", 24)], None),
    );
    let decision = eval(&rules, &request);
    assert_eq!(decision.outcome, Allow);
    assert!(decision.hard_limits_applied.is_empty());
}

#[test]
fn ceiling_floors_refuse_a_target_wider_than_the_family_floor() {
    let fx = Fx::new();
    let rules = [rule("allow", Allow)];
    let both = ceiling_with(&[("prefix_len_v4", 24), ("prefix_len_v6", 64)], None);
    // A /16 IPv4 target is wider than the floor of 24.
    let wide_v4 = block(&fx, 16, 128, 600, both.clone());
    assert_eq!(
        eval(&rules, &wide_v4).hard_limits_applied,
        vec![HardLimit::CeilingDoesNotCover]
    );
    // A /48 IPv6 target is wider than the floor of 64.
    let wide_v6 = block(&fx, 32, 48, 600, both.clone());
    assert_eq!(
        eval(&rules, &wide_v6).hard_limits_applied,
        vec![HardLimit::CeilingDoesNotCover]
    );
    // Exactly at each floor is covered.
    assert_eq!(
        eval(&rules, &block(&fx, 24, 128, 600, both.clone())).outcome,
        Allow
    );
    assert_eq!(eval(&rules, &block(&fx, 32, 64, 600, both)).outcome, Allow);
}

#[test]
fn ceiling_with_a_floor_does_not_cover_a_request_missing_the_argument() {
    let fx = Fx::new();
    let rules = [rule("allow", Allow)];
    let mut request = block(
        &fx,
        32,
        128,
        600,
        ceiling_with(&[("prefix_len_v6", 64)], None),
    );
    request.args = CanonicalArgs::from_canonical("prefix_len_v4:2:32\n");
    assert_eq!(
        eval(&rules, &request).hard_limits_applied,
        vec![HardLimit::CeilingDoesNotCover],
        "a missing bounded argument is never covered"
    );
}

#[test]
fn ceiling_ttl_bound_is_checked_against_the_requested_ttl() {
    let fx = Fx::new();
    let rules = [rule("allow", Allow)];
    let capped = ceiling_with(&[], Some(SignedDuration::from_secs(3600)));
    assert_eq!(
        eval(&rules, &block(&fx, 32, 128, 3600, capped.clone())).outcome,
        Allow
    );
    assert_eq!(
        eval(&rules, &block(&fx, 32, 128, 3601, capped)).hard_limits_applied,
        vec![HardLimit::CeilingDoesNotCover]
    );
}
