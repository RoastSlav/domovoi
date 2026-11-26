//! Behaviour that needs a real kernel and `nft`, but no packet load.

use domovoi_core::containment::templates::{Seconds, SetName, TableName, delete_own_table};
use domovoi_core::containment::{Layout, Net, Token, templates};
use domovoi_helper::nft::journal_file::fold;
use domovoi_helper::nft::{OUT_COVERING, OUT_OUTCOME, apply, classify, confirm, ensure, rollback};
use uuid::Uuid;

use super::support::{Ns, OPEN, enabled, eventually, raw, state, unverified};

const HOST: &str = "203.0.113.7/32";
const PREFIX: &str = "203.0.113.64/26";

fn block(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

macro_rules! ns_test {
    ($name:ident, $ns:ident, $cx:ident, $body:block) => {
        #[tokio::test]
        async fn $name() {
            if !enabled(stringify!($name)) {
                return;
            }
            let $ns = Ns::new().await;
            let lock = domovoi_helper::state::StateLock::open($ns.dir.path()).expect("lock");
            let guard = lock.acquire().expect("guard");
            let $cx = $ns.cx(&guard, 1000, 10);
            let _ = &$cx;
            $body
        }
    };
}

async fn members(cx: &domovoi_helper::nft::Cx<'_>, set: &str) -> usize {
    let view = ensure::read_table(cx).await.expect("read").expect("table");
    view.sets[set].members.len()
}

ns_test!(
    kernel_timeout_expires_element_after_helper_sigkill,
    ns,
    cx,
    {
        // The helper is not running at all while the kernel's timer fires: the harness creates the
        // element directly with a short timeout and only polls.
        let _ = state(&cx, OPEN).await;
        let (exit, _) = raw(
            &ns,
            "create element inet domovoi blocked4 { 203.0.113.9 timeout 3s }\n",
        )
        .await;
        assert_eq!(exit, Some(0));
        assert!(eventually(async || members(&cx, "blocked4").await == 0).await);
    }
);

ns_test!(
    create_existing_element_reports_already_present_and_keeps_ttl,
    ns,
    cx,
    {
        let st = state(&cx, OPEN).await;
        raw(
            &ns,
            "create element inet domovoi blocked4 { 203.0.113.7 timeout 20s }\n",
        )
        .await;
        let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
            .await
            .expect_err("already there");
        assert_eq!(err.token, Token::AlreadyPresent);
        let view = ensure::read_table(&cx).await.expect("read").expect("table");
        let left = view.sets["blocked4"].members[0].expires.expect("expires");
        assert!(
            (1..=20).contains(&left),
            "the TTL was not changed to 30: {left}"
        );
    }
);

ns_test!(
    add_on_existing_element_behaviour_is_recorded_per_kernel,
    ns,
    cx,
    {
        let _ = state(&cx, OPEN).await;
        raw(
            &ns,
            "create element inet domovoi blocked4 { 203.0.113.7 timeout 30s }\n",
        )
        .await;
        let (exit, stderr) = raw(
            &ns,
            "add element inet domovoi blocked4 { 203.0.113.7 timeout 10s }\n",
        )
        .await;
        let view = ensure::read_table(&cx).await.expect("read").expect("table");
        let left = view.sets["blocked4"].members[0].expires.expect("expires");
        // Recorded, never relied on: a refresh, a no-op or a shortening are all possible.
        eprintln!("RECORDED add on an existing element: exit {exit:?}, {left}s left, {stderr:?}");
        assert!(left >= 1);
    }
);

ns_test!(
    step_one_create_succeeds_on_absent_plain_and_interval_prefix,
    ns,
    cx,
    {
        let st = state(&cx, OPEN).await;
        for (n, source, set) in [
            (1, HOST, "blocked4"),
            (2, PREFIX, "blockednet4"),
            (3, "2001:db8::7/128", "blocked6"),
        ] {
            let out = apply::apply(&cx, &st, block(n), source, 600, Some(&unverified()))
                .await
                .unwrap_or_else(|e| panic!("{source}: {e:?}"));
            assert_eq!(out["set"], set);
            let st2 = state(&cx, OPEN).await;
            assert!(st2.ens.drift.is_none());
        }
    }
);

ns_test!(
    rollback_of_absent_element_succeeds_on_plain_and_interval_sets,
    ns,
    cx,
    {
        let st = state(&cx, OPEN).await;
        for source in [HOST, PREFIX, "203.0.113.0/24"] {
            let out = rollback::rollback(&cx, &st, block(9), source, 600)
                .await
                .expect(source);
            assert!(
                matches!(out[OUT_OUTCOME].as_str(), "rolled_back" | "absent"),
                "{source}"
            );
        }
    }
);

#[tokio::test]
async fn rollback_on_full_set_falls_back_to_membership_read() {
    if !enabled("rollback_on_full_set_falls_back_to_membership_read") {
        return;
    }
    let ns = Ns::new().await;
    let lock = domovoi_helper::state::StateLock::open(ns.dir.path()).expect("lock");
    let guard = lock.acquire().expect("guard");
    let cx = ns.cx(&guard, 1, 10);
    let st = state(&cx, OPEN).await;
    raw(
        &ns,
        "create element inet domovoi blocked4 { 203.0.113.200 timeout 600s }\n",
    )
    .await;
    let out = rollback::rollback(&cx, &st, block(9), HOST, 600)
        .await
        .expect("an absent element on a full set is a success");
    assert_eq!(out[OUT_OUTCOME], "absent");
    assert_eq!(
        members(&cx, "blocked4").await,
        1,
        "the foreign element is untouched"
    );
}

ns_test!(rollback_with_covering_prefix_reports_covered, ns, cx, {
    let st = state(&cx, OPEN).await;
    raw(
        &ns,
        "create element inet domovoi blockednet4 { 203.0.113.0/24 timeout 600s }\n",
    )
    .await;
    let out = rollback::rollback(&cx, &st, block(9), PREFIX, 600)
        .await
        .expect("rollback");
    assert_eq!(out[OUT_OUTCOME], "absent_covered");
    assert!(out[OUT_COVERING].starts_with("203.0.113.0"));
    assert_eq!(
        members(&cx, "blockednet4").await,
        1,
        "the covering element is untouched"
    );
});

ns_test!(
    replace_of_expired_element_fails_and_does_not_recreate,
    ns,
    cx,
    {
        let _ = state(&cx, OPEN).await;
        raw(
            &ns,
            "create element inet domovoi blocked4 { 203.0.113.7 timeout 2s }\n",
        )
        .await;
        assert!(eventually(async || members(&cx, "blocked4").await == 0).await);
        let table = TableName::parse("domovoi").expect("table");
        let net = Net::parse(HOST).expect("net");
        let script =
            templates::confirm(&table, &net, Seconds::new(600).expect("ttl")).expect("confirm");
        let (exit, stderr) = raw(&ns, &script).await;
        assert_eq!(exit, Some(1), "{stderr}");
        assert_eq!(
            members(&cx, "blocked4").await,
            0,
            "the failed batch did not re-create it"
        );
    }
);

ns_test!(
    replace_losing_to_rollback_is_classified_rolled_back_from_journal,
    ns,
    cx,
    {
        let table = TableName::parse("domovoi").expect("table");
        let net = Net::parse(HOST).expect("net");
        let confirm_script =
            templates::confirm(&table, &net, Seconds::new(600).expect("ttl")).expect("confirm");
        let rollback_script = templates::rollback(&table, &net);
        for round in 0..15u128 {
            let st = state(&cx, OPEN).await;
            let id = block(100 + round);
            apply::apply(&cx, &st, id, HOST, 600, Some(&unverified()))
                .await
                .expect("apply");
            cx.journal
                .append(
                    cx.guard,
                    &domovoi_core::containment::JournalRecord::RollbackIntent {
                        block: id.to_string(),
                        at: cx.clock.now(),
                    },
                )
                .expect("journal");
            let (a, b) = tokio::join!(raw(&ns, &confirm_script), raw(&ns, &rollback_script));
            let _ = (a, b);
            let blocks = fold(&cx.journal.load().expect("journal").records);
            let known = blocks
                .iter()
                .find(|b| b.block == id.to_string())
                .expect("block");
            let (verdict, _) = classify::absent(&cx, &net, Some(known), cx.clock.now())
                .await
                .expect("absent");
            assert_ne!(
                verdict,
                domovoi_core::containment::AbsentClass::Expired,
                "round {round}: a removal the journal explains is never expired"
            );
            let _ = raw(&ns, &rollback_script).await;
        }
    }
);

ns_test!(bare_no_such_file_is_table_missing_not_expired, ns, cx, {
    let st = state(&cx, OPEN).await;
    apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    let st = state(&cx, OPEN).await;
    raw(
        &ns,
        &delete_own_table(&TableName::parse("domovoi").expect("table")),
    )
    .await;
    let err = confirm::confirm(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("confirm");
    assert_eq!(err.token, Token::ContainmentLost);
});

ns_test!(
    flush_ruleset_reports_containment_lost_and_recreates_empty_table,
    ns,
    cx,
    {
        let st = state(&cx, OPEN).await;
        apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
            .await
            .expect("apply");
        raw(&ns, "flush ruleset\n").await;
        let st = state(&cx, OPEN).await;
        assert_eq!(st.ens.lost, [block(1).to_string()]);
        assert_eq!(members(&cx, "blocked4").await, 0);
    }
);

ns_test!(bootstrap_rerun_does_not_duplicate_rules, ns, cx, {
    let st = state(&cx, OPEN).await;
    let again = templates::bootstrap(
        &TableName::parse("domovoi").expect("table"),
        Layout::HostsAndPrefixes,
        domovoi_core::containment::templates::MaxElements::new(1000).expect("max"),
    );
    let (exit, stderr) = raw(&ns, &again).await;
    assert_eq!(
        exit,
        Some(1),
        "a second bootstrap starts with create table: {stderr}"
    );
    assert!(stderr.contains("File exists"));
    let after = state(&cx, OPEN).await;
    assert_eq!(after.ens.view.rules.len(), st.ens.view.rules.len());
});

ns_test!(
    ensure_reports_table_drift_when_set_size_or_default_timeout_differs,
    ns,
    cx,
    {
        let _ = state(&cx, OPEN).await;
        // An `add set` over a live table silently rewrites the size: exactly why the helper never sends one.
        let (exit, _) = raw(&ns, "add set inet domovoi blocked4 { type ipv4_addr; flags timeout; timeout 30s; size 5; }\n").await;
        assert_eq!(exit, Some(0));
        let st = state(&cx, OPEN).await;
        assert!(st.ens.drift.is_some());
    }
);

ns_test!(
    layout_change_on_an_empty_table_recreates_it_and_on_a_used_table_drifts,
    ns,
    cx,
    {
        let hosts = state(&cx, super::support::OPEN).await;
        drop(hosts);
        let st = state(
            &cx,
            domovoi_core::containment::Gate::Open {
                interval_path: false,
            },
        )
        .await;
        assert!(!st.ens.view.sets.contains_key("blockednet4"));
        let st = state(&cx, OPEN).await;
        assert_eq!(st.ens.state.as_str(), "recreated");
        apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
            .await
            .expect("apply");
        let used = state(
            &cx,
            domovoi_core::containment::Gate::Open {
                interval_path: false,
            },
        )
        .await;
        assert!(used.ens.drift.is_some());
    }
);

ns_test!(exclusion_sets_accept_coalesced_ranges, ns, cx, {
    let st = state(&cx, OPEN).await;
    let excl = &st.ens.view.sets["excl4"].members;
    assert!(
        excl.iter()
            .any(|m| (m.lo, m.hi) == (0xC000_0200, 0xC000_02FF))
    );
    assert!(st.ens.drift.is_none());
});

ns_test!(element_without_timeout_takes_set_default, ns, cx, {
    let _ = state(&cx, OPEN).await;
    raw(&ns, "add element inet domovoi blocked4 { 203.0.113.9 }\n").await;
    let view = ensure::read_table(&cx).await.expect("read").expect("table");
    let left = view.sets["blocked4"].members[0]
        .expires
        .expect("the set's default timeout applies");
    assert!((1..=30).contains(&left));
});

#[tokio::test]
async fn max_active_counts_foreign_elements_from_json() {
    if !enabled("max_active_counts_foreign_elements_from_json") {
        return;
    }
    let ns = Ns::new().await;
    let lock = domovoi_helper::state::StateLock::open(ns.dir.path()).expect("lock");
    let guard = lock.acquire().expect("guard");
    let cx = ns.cx(&guard, 1000, 1);
    let _ = state(&cx, OPEN).await;
    raw(
        &ns,
        "create element inet domovoi blocked4 { 198.51.100.9 timeout 600s }\n",
    )
    .await;
    let st = state(&cx, OPEN).await;
    let err = apply::apply(&cx, &st, block(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("the foreign element fills the budget");
    assert_eq!(err.token, Token::MaxActiveReached);
}

ns_test!(
    verification_probe_detects_a_reordered_or_rehooked_chain,
    ns,
    cx,
    {
        let _ = state(&cx, OPEN).await;
        raw(&ns, "flush chain inet domovoi contain\nadd rule inet domovoi contain ip saddr @blocked4 counter drop\nadd rule inet domovoi contain counter\n").await;
        let st = state(&cx, OPEN).await;
        assert!(st.ens.drift.is_some(), "a reordered chain is drift");
        let set = SetName::for_net(&Net::parse(HOST).expect("net"));
        assert_eq!(set.as_str(), "blocked4");
    }
);
