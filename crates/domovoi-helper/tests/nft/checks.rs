//! What the helper refuses, for every caller, before any element changes.

use domovoi_core::attest::{ConfigLines, RouteLines};
use domovoi_core::containment::{Gate, Token, UntestedReason};
use domovoi_helper::attest::current::Current;
use domovoi_helper::nft::{Refusal, apply, confirm, exclusions_of, rollback};
use uuid::Uuid;

use super::probe::{count, seed};
use super::world::{OPEN, OPEN_HOSTS, World, facts, identity, no_sys, state, unverified, verified};

const HOST: &str = "203.0.113.7/32";

fn block(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

macro_rules! cx {
    ($w:ident, $cx:ident) => {
        let lock = $w.lock();
        let guard = lock.acquire().expect("lock");
        let sys = no_sys();
        let $cx = $w.cx(&guard, &sys);
    };
}

#[tokio::test]
async fn expected_safety_missing_is_refused_malformed_safety() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    for result in [
        apply::apply(&cx, &st, block(1), HOST, 600, None).await,
        confirm::confirm(&cx, &st, block(1), HOST, 600, None).await,
    ] {
        assert_eq!(result.expect_err("refused").token, Token::MalformedSafety);
    }
    assert_eq!(count(&w, "blocked4"), 0);
}

#[tokio::test]
async fn verified_identity_mismatch_is_safety_changed() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&verified(2)))
        .await
        .expect_err("refused");
    assert_eq!(err.token, Token::SafetyChanged);
    let mut unknown = facts(OPEN);
    unknown.identity = Err(());
    let st = state(&cx, unknown).await;
    let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&verified(1)))
        .await
        .expect_err("refused");
    assert_eq!(err.token, Token::SafetyUnknown);
    assert_eq!(identity(1).generation, 1);
}

#[tokio::test]
async fn unverified_skips_only_the_identity_comparison() {
    let w = World::new();
    cx!(w, cx);
    let mut unknown = facts(OPEN);
    unknown.identity = Err(());
    let st = state(&cx, unknown).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("an unverified approval needs no identity");
    let st = state(&cx, facts(OPEN)).await;
    let err = apply::apply(&cx, &st, block(2), "192.0.2.9/32", 600, Some(&unverified()))
        .await
        .expect_err("overlap");
    assert_eq!(err.token, Token::ExclusionOverlap);
}

#[tokio::test]
async fn helper_refuses_target_overlapping_exclusion_even_if_agent_requests() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    for source in [
        "192.0.2.7/32",
        "192.0.0.0/16",
        "127.0.0.1/32",
        "169.254.1.1/32",
    ] {
        let err = apply::apply(&cx, &st, block(1), source, 600, Some(&verified(1)))
            .await
            .expect_err(source);
        assert!(
            matches!(err.token, Token::ExclusionOverlap | Token::PrefixTooWide),
            "{source}: {:?}",
            err.token
        );
    }
    assert_eq!(count(&w, "blocked4"), 0);
}

#[tokio::test]
async fn helper_refuses_overlap_without_expected_safety() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    let err = apply::apply(&cx, &st, block(1), "192.0.2.7/32", 600, None)
        .await
        .expect_err("refused");
    assert_eq!(err.token, Token::MalformedSafety);
}

#[tokio::test]
async fn targets_are_parsed_strictly_and_floors_apply_to_every_caller() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    let cases = [
        ("evil.example", Token::InvalidTarget),
        ("203.0.113.7", Token::InvalidTarget),
        ("203.0.113.7/24", Token::InvalidTarget),
        ("::ffff:203.0.113.7/128", Token::MappedForm),
        ("0.0.0.0/0", Token::PrefixTooWide),
        ("203.0.0.0/16", Token::PrefixTooWide),
        ("2001:db8::/32", Token::PrefixTooWide),
    ];
    for (source, token) in cases {
        let err = apply::apply(&cx, &st, block(1), source, 600, Some(&unverified()))
            .await
            .expect_err(source);
        assert_eq!(err.token, token, "{source}");
    }
}

#[tokio::test]
async fn ttl_is_refused_not_clamped() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    for (ttl, token) in [
        (0, Token::TtlTooShort),
        (30, Token::TtlTooShort),
        (3601, Token::TtlAboveMaximum),
        (31_536_001, Token::TtlAboveMaximum),
    ] {
        let err = apply::apply(&cx, &st, block(1), HOST, ttl, Some(&unverified()))
            .await
            .expect_err("ttl");
        assert_eq!(err.token, token, "{ttl}");
    }
    assert!(!w.scripts().iter().any(|s| s.contains("3601s")));
    assert_eq!(count(&w, "blocked4"), 0);
}

#[tokio::test]
async fn max_active_zero_refuses_everything_and_foreign_elements_count() {
    let mut w = World::new();
    w.max_active = 0;
    {
        cx!(w, cx);
        let st = state(&cx, facts(OPEN)).await;
        let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
            .await
            .expect_err("max_active 0");
        assert_eq!(err.token, Token::MaxActiveReached);
    }
    let mut w = World::new();
    w.max_active = 1;
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    seed(&w, "blocked4", "198.51.100.9", 600);
    let st2 = state(&cx, facts(OPEN)).await;
    let _ = st;
    let err = apply::apply(&cx, &st2, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("a foreign element fills the budget");
    assert_eq!(err.token, Token::MaxActiveReached);
}

#[tokio::test]
async fn max_active_counts_interval_sets_when_the_interval_path_is_on() {
    let mut w = World::new();
    w.max_active = 1;
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    seed(&w, "blockednet4", "198.51.100.0/24", 600);
    let st2 = state(&cx, facts(OPEN)).await;
    let _ = st;
    let err = apply::apply(&cx, &st2, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("a prefix counts");
    assert_eq!(err.token, Token::MaxActiveReached);
}

#[tokio::test]
async fn gate_blocks_apply_and_confirm_when_leak_state_is_not_open() {
    let w = World::new();
    cx!(w, cx);
    for gate in [
        Gate::BlockedLeak,
        Gate::BlockedUntested {
            reason: UntestedReason::NeverRun,
        },
    ] {
        let st = state(&cx, facts(gate)).await;
        for result in [
            apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified())).await,
            confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified())).await,
        ] {
            assert_eq!(
                result.expect_err("blocked").token,
                Token::ContainmentBlocked
            );
        }
    }
    assert_eq!(count(&w, "blocked4"), 0);
}

#[tokio::test]
async fn prefix_target_is_refused_when_interval_path_is_disabled() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN_HOSTS)).await;
    let err = apply::apply(
        &cx,
        &st,
        block(1),
        "203.0.113.64/26",
        600,
        Some(&unverified()),
    )
    .await
    .expect_err("prefix");
    assert_eq!(err.token, Token::PrefixUnsupportedPlatform);
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("a single host is still fine");
}

#[tokio::test]
async fn mutation_limit_counts_apply_only() {
    let mut w = World::new();
    w.mutations_per_hour = 1;
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("the first apply");
    let st = state(&cx, facts(OPEN)).await;
    confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("confirm spends nothing");
    rollback::rollback(&cx, &st, block(1), HOST, 600)
        .await
        .expect("rollback spends nothing");
    let st = state(&cx, facts(OPEN)).await;
    let err = apply::apply(
        &cx,
        &st,
        block(2),
        "203.0.113.8/32",
        600,
        Some(&unverified()),
    )
    .await
    .expect_err("the second apply");
    assert_eq!(err.token, Token::MutationLimit);
    w.advance(3601);
}

#[tokio::test]
async fn mutations_per_hour_zero_refuses_every_apply() {
    let mut w = World::new();
    w.mutations_per_hour = 0;
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("limit 0");
    assert_eq!(err.token, Token::MutationLimit);
}

#[tokio::test]
async fn rollback_is_never_refused_for_a_safety_reason() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    let mut hostile = facts(Gate::BlockedLeak);
    hostile.exclusions = Err(Refusal::new(Token::NoManagementCidrs, "none"));
    hostile.identity = Err(());
    let mut st = state(&cx, hostile).await;
    st.ens.drift = Some("tampered".to_owned());
    rollback::rollback(&cx, &st, block(1), HOST, 600)
        .await
        .expect("rollback skips the gate, the exclusions, the identity and drift");
    assert_eq!(count(&w, "blocked4"), 0);
    let err = apply::apply(&cx, &st, block(2), HOST, 600, Some(&unverified()))
        .await
        .expect_err("every other operation still checks");
    assert_eq!(err.token, Token::TableDrift);
}

#[test]
fn no_management_cidrs_refuses_containment() {
    let none = Current {
        config: Ok(ConfigLines::default()),
        routes: Ok(RouteLines::default()),
        identity: Err(()),
    };
    assert_eq!(
        exclusions_of(&none).expect_err("no CIDR").token,
        Token::NoManagementCidrs
    );
    let unreadable = Current {
        config: Ok(ConfigLines::default()),
        routes: Err(()),
        identity: Err(()),
    };
    assert_eq!(
        exclusions_of(&unreadable).expect_err("no routes").token,
        Token::ExclusionsIncomplete
    );
}

#[test]
fn exclusion_inputs_come_from_the_observed_lines() {
    let config = ConfigLines {
        cidr: vec!["198.51.100.0/24".to_owned()],
        ..ConfigLines::default()
    };
    let routes = RouteLines {
        hub_addr: vec!["192.0.2.77".to_owned()],
        ..RouteLines::default()
    };
    let current = Current {
        config: Ok(config),
        routes: Ok(routes),
        identity: Err(()),
    };
    let set = exclusions_of(&current).expect("exclusions");
    let covers = |text: &str| {
        let net = domovoi_core::containment::Net::parse(text).expect("net");
        set.iter().any(|e| e.overlaps(&net))
    };
    assert!(covers("198.51.100.9/32"));
    assert!(covers("192.0.2.77/32"));
    assert!(covers("127.0.0.1/32"));
    assert!(!covers("203.0.113.7/32"));
}
