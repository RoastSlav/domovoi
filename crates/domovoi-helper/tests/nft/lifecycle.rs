//! The three phases of a block against the fake `nft`.

use domovoi_core::containment::Token;
use domovoi_helper::nft::{OUT_COVERING, OUT_OUTCOME, State, apply, confirm, list, rollback};
use uuid::Uuid;

use super::probe::{count, left, seed};
use super::world::{OPEN, World, facts, no_sys, state, unverified, verified};

const HOST: &str = "203.0.113.7/32";
const PREFIX: &str = "203.0.113.64/26";

fn block(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

#[tokio::test]
async fn apply_confirm_and_rollback_move_the_element_through_its_phases() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    let out = apply::apply(&cx, &st, block(1), HOST, 600, Some(&verified(1)))
        .await
        .expect("apply");
    assert_eq!(out["phase"], "apply");
    assert_eq!(out["set"], "blocked4");
    assert_eq!(left(&w, "blocked4"), [30]);
    w.advance(5);
    let st = state(&cx, facts(OPEN)).await;
    let out = confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&verified(1)))
        .await
        .expect("confirm");
    assert_eq!(out["remaining_s"], "600");
    assert_eq!(left(&w, "blocked4"), [600]);
    let st = state(&cx, facts(OPEN)).await;
    let out = rollback::rollback(&cx, &st, block(1), HOST, 600)
        .await
        .expect("rollback");
    assert_eq!(out[OUT_OUTCOME], "rolled_back");
    assert_eq!(count(&w, "blocked4"), 0);
}

#[tokio::test]
async fn unconfirmed_block_expires_after_the_step_one_timeout() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    w.advance(31);
    assert_eq!(count(&w, "blocked4"), 0, "the kernel timeout removed it");
    let st = state(&cx, facts(OPEN)).await;
    let err = confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("confirm");
    assert_eq!(err.token, Token::Expired);
}

#[tokio::test]
async fn replace_of_expired_element_fails_and_does_not_recreate() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    w.advance(40);
    let st = state(&cx, facts(OPEN)).await;
    let _ = confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified())).await;
    assert_eq!(
        count(&w, "blocked4"),
        0,
        "the delete failed and took the add with it"
    );
}

#[tokio::test]
async fn create_existing_element_reports_already_present_and_keeps_ttl() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    seed(&w, "blocked4", "203.0.113.7", 20);
    let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("apply");
    assert_eq!(err.token, Token::AlreadyPresent);
    assert_eq!(left(&w, "blocked4"), [20], "the TTL did not change");
}

#[tokio::test]
async fn a_repeated_apply_of_the_same_block_is_idempotent() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply again");
    assert_eq!(count(&w, "blocked4"), 1);
}

#[tokio::test]
async fn rollback_of_absent_element_succeeds_on_plain_and_interval_sets() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    for source in [HOST, PREFIX] {
        let out = rollback::rollback(&cx, &st, block(9), source, 600)
            .await
            .expect("rollback");
        assert!(matches!(
            out[OUT_OUTCOME].as_str(),
            "rolled_back" | "absent"
        ));
    }
}

#[tokio::test]
async fn rollback_on_full_set_falls_back_to_membership_read() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    {
        let mut nft = w.nft.lock().expect("nft");
        nft.table("domovoi")
            .sets
            .get_mut("blocked4")
            .expect("set")
            .size = Some(1);
    }
    seed(&w, "blocked4", "203.0.113.200", 600);
    let out = rollback::rollback(&cx, &st, block(9), HOST, 600)
        .await
        .expect("rollback of an absent element on a full set");
    assert_eq!(out[OUT_OUTCOME], "absent");
    assert_eq!(count(&w, "blocked4"), 1, "the foreign element is untouched");
}

#[tokio::test]
async fn rollback_with_covering_prefix_reports_covered_and_never_deletes_it() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    seed(&w, "blockednet4", "203.0.113.0/24", 600);
    let out = rollback::rollback(&cx, &st, block(9), PREFIX, 600)
        .await
        .expect("rollback");
    assert_eq!(out[OUT_OUTCOME], "absent_covered");
    assert!(out[OUT_COVERING].contains("203.0.113.0"));
    assert_eq!(count(&w, "blockednet4"), 1);
    assert!(
        !w.scripts()
            .iter()
            .any(|s| s.starts_with("delete element") && s.contains("203.0.113.0/24")),
        "the covering element is never deleted"
    );
}

#[tokio::test]
async fn confirm_names_unknown_and_mismatched_blocks() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    let err = confirm::confirm(&cx, &st, block(5), HOST, 600, Some(&unverified()))
        .await
        .expect_err("unknown");
    assert_eq!(err.token, Token::UnknownBlock);
    apply::apply(&cx, &st, block(5), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    let st = state(&cx, facts(OPEN)).await;
    let err = confirm::confirm(&cx, &st, block(5), HOST, 900, Some(&unverified()))
        .await
        .expect_err("mismatch");
    assert_eq!(err.token, Token::BlockMismatch);
}

#[tokio::test]
async fn bare_no_such_file_is_table_missing_not_expired() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st: State = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    let st = state(&cx, facts(OPEN)).await;
    w.nft.lock().expect("nft").tables.clear();
    let err = confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("confirm");
    assert_eq!(err.token, Token::ContainmentLost);
}

#[tokio::test]
async fn flush_ruleset_reports_containment_lost_and_recreates_empty_table() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    w.nft.lock().expect("nft").tables.clear();
    let st = state(&cx, facts(OPEN)).await;
    assert_eq!(st.ens.lost, [block(1).to_string()]);
    assert_eq!(st.ens.state.as_str(), "recreated");
    assert_eq!(count(&w, "blocked4"), 0, "nothing was re-applied");
    let out = list::list(&cx, &st).await.expect("list");
    assert_eq!(out["lost_blocks"], block(1).to_string());
    let err = rollback::rollback(&cx, &st, block(1), HOST, 600)
        .await
        .expect_err("rollback of a lost block");
    assert_eq!(err.token, Token::ContainmentLost);
}

#[tokio::test]
async fn list_reports_orphans_and_counts_them() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    seed(&w, "blocked4", "198.51.100.9", 25);
    let st2 = state(&cx, facts(OPEN)).await;
    let _ = st;
    let out = list::list(&cx, &st2).await.expect("list");
    assert_eq!(out["active"], "1");
    assert!(out["orphans"].contains("198.51.100.9"));
}
