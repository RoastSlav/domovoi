//! Named behaviours of the `nft` group that other documents refer to by name: the helper counts
//! what is in the sets, a TTL above the maximum is refused, and only `rollback` skips a check.

use domovoi_core::containment::{Gate, Token, keys};
use domovoi_helper::nft::{Refusal, apply, confirm, list, rollback};
use uuid::Uuid;

use super::probe::{count, seed};
use super::world::{OPEN, World, facts, no_sys, state, unverified, verified};

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
async fn helper_active_count_is_authoritative() {
    let mut w = World::new();
    w.max_active = 2;
    cx!(w, cx);
    // Two elements the helper never journaled: whatever the caller believes, the sets decide.
    let _ = state(&cx, facts(OPEN)).await;
    seed(&w, "blocked4", "198.51.100.9", 600);
    seed(&w, "blocked4", "198.51.100.10", 600);
    let st = state(&cx, facts(OPEN)).await;
    let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("the sets are already full to the helper's limit");
    assert_eq!(err.token, Token::MaxActiveReached);
    assert_eq!(count(&w, "blocked4"), 2, "nothing was added");
    let listed = list::list(&cx, &st).await.expect("list");
    assert_eq!(
        listed.get(keys::OUT_ACTIVE).map(String::as_str),
        Some("2"),
        "the listing reports the JSON element count"
    );
}

#[tokio::test]
async fn ttl_above_maximum_refused_not_clamped() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    let above = u32::try_from(w.max_ttl_s + 1).expect("fits");
    let err = apply::apply(&cx, &st, block(1), HOST, above, Some(&unverified()))
        .await
        .expect_err("above the helper maximum");
    assert_eq!(err.token, Token::TtlAboveMaximum);
    assert_eq!(
        count(&w, "blocked4"),
        0,
        "no shorter block was made instead"
    );
    let clamped = format!("{}s", w.max_ttl_s);
    assert!(
        !w.scripts().iter().any(|s| s.contains(&clamped)),
        "no script carries the maximum in place of the request"
    );
    apply::apply(&cx, &st, block(2), HOST, 600, Some(&unverified()))
        .await
        .expect("a TTL inside the window is accepted");
    let st = state(&cx, facts(OPEN)).await;
    let err = confirm::confirm(&cx, &st, block(2), HOST, above, Some(&unverified()))
        .await
        .expect_err("confirm applies the same maximum");
    assert_eq!(err.token, Token::TtlAboveMaximum);
}

/// The facts under which an operation must refuse, and the token it refuses with.
fn spoiled(which: &str) -> (domovoi_helper::nft::Facts, Token) {
    let mut f = facts(OPEN);
    match which {
        "gate" => {
            f.gate = Gate::BlockedLeak;
            (f, Token::ContainmentBlocked)
        }
        _ => {
            f.exclusions = Err(Refusal::new(Token::NoManagementCidrs, "none"));
            (f, Token::NoManagementCidrs)
        }
    }
}

#[tokio::test]
async fn no_operation_but_rollback_skips_a_check() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    for name in ["gate", "exclusions"] {
        let (f, token) = spoiled(name);
        let st = state(&cx, f).await;
        let err = apply::apply(
            &cx,
            &st,
            block(9),
            "203.0.113.9/32",
            600,
            Some(&unverified()),
        )
        .await
        .expect_err(name);
        assert_eq!(err.token, token, "apply under {name}");
        let err = confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified()))
            .await
            .expect_err(name);
        assert_eq!(err.token, token, "confirm under {name}");
    }
    // The identity check and the drift check hold for both as well.
    let mut f = facts(OPEN);
    f.identity = Err(());
    let st = state(&cx, f).await;
    for result in [
        apply::apply(
            &cx,
            &st,
            block(9),
            "203.0.113.9/32",
            600,
            Some(&verified(1)),
        )
        .await,
        confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&verified(1))).await,
    ] {
        assert_eq!(result.expect_err("identity").token, Token::SafetyUnknown);
    }
    let mut st = state(&cx, facts(OPEN)).await;
    st.ens.drift = Some("tampered".to_owned());
    for result in [
        apply::apply(
            &cx,
            &st,
            block(9),
            "203.0.113.9/32",
            600,
            Some(&unverified()),
        )
        .await,
        confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified())).await,
    ] {
        assert_eq!(result.expect_err("drift").token, Token::TableDrift);
    }
    // A missing `expected_safety` is refused by both.
    let st = state(&cx, facts(OPEN)).await;
    for result in [
        apply::apply(&cx, &st, block(9), "203.0.113.9/32", 600, None).await,
        confirm::confirm(&cx, &st, block(1), HOST, 600, None).await,
    ] {
        assert_eq!(result.expect_err("safety").token, Token::MalformedSafety);
    }
    // Under every one of those states the removal still goes through.
    let mut hostile = facts(Gate::BlockedLeak);
    hostile.exclusions = Err(Refusal::new(Token::NoManagementCidrs, "none"));
    hostile.identity = Err(());
    let mut st = state(&cx, hostile).await;
    st.ens.drift = Some("tampered".to_owned());
    rollback::rollback(&cx, &st, block(1), HOST, 600)
        .await
        .expect("rollback is the one operation that skips them");
    assert_eq!(count(&w, "blocked4"), 0);
}
