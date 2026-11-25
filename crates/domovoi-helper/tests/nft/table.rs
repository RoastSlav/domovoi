//! `ensure_table`: create when absent, compare when present, drift, layout, exclusions, flows
//! and the verification probe.

use domovoi_core::containment::Token;
use domovoi_helper::nft::flows::bridge_gap;
use domovoi_helper::nft::{apply, rollback};
use uuid::Uuid;

use super::probe::count;
use super::world::{OPEN, OPEN_HOSTS, World, facts, no_sys, state, unverified};

const HOST: &str = "203.0.113.7/32";

macro_rules! cx {
    ($w:ident, $cx:ident) => {
        let lock = $w.lock();
        let guard = lock.acquire().expect("lock");
        let sys = no_sys();
        let $cx = $w.cx(&guard, &sys);
    };
}

fn script_count(w: &World) -> usize {
    w.scripts().len()
}

#[tokio::test]
async fn bootstrap_over_a_live_table_sends_nothing_when_the_json_matches() {
    let w = World::new();
    cx!(w, cx);
    let first = state(&cx, facts(OPEN)).await;
    assert_eq!(first.ens.state.as_str(), "created");
    let sent = script_count(&w);
    let second = state(&cx, facts(OPEN)).await;
    assert_eq!(second.ens.state.as_str(), "ok");
    assert!(second.ens.drift.is_none());
    assert_eq!(script_count(&w), sent, "a matching table needs no script");
}

#[tokio::test]
async fn bootstrap_never_sends_add_set_add_chain_or_add_rule_over_a_live_table() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, Uuid::from_u128(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    for _ in 0..3 {
        let _ = state(&cx, facts(OPEN)).await;
    }
    let scripts = w.scripts();
    let adds: Vec<usize> = scripts
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            ["add set", "add chain", "add rule"]
                .iter()
                .any(|n| s.contains(n))
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        adds,
        [0],
        "only the first script, the bootstrap, builds structure"
    );
    assert!(scripts[0].starts_with("create table inet domovoi\n"));
}

#[tokio::test]
async fn ensure_reports_table_drift_when_set_size_or_default_timeout_differs() {
    for tamper in 0..2 {
        let w = World::new();
        cx!(w, cx);
        let _ = state(&cx, facts(OPEN)).await;
        {
            let mut nft = w.nft.lock().expect("nft");
            let set = nft.table("domovoi").sets.get_mut("blocked4").expect("set");
            if tamper == 0 {
                set.size = Some(5);
            } else {
                set.timeout = Some(3600);
            }
        }
        let st = state(&cx, facts(OPEN)).await;
        assert!(st.ens.drift.is_some(), "tamper {tamper}");
        let err = apply::apply(&cx, &st, Uuid::from_u128(1), HOST, 600, Some(&unverified()))
            .await
            .expect_err("drift");
        assert_eq!(err.token, Token::TableDrift);
    }
}

#[tokio::test]
async fn verification_probe_detects_a_reordered_or_rehooked_chain() {
    let w = World::new();
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    {
        let mut nft = w.nft.lock().expect("nft");
        nft.table("domovoi").chain = Some(("input".to_owned(), 0, "accept".to_owned()));
    }
    // The table now differs from the template, so a fresh `ensure` reports drift; the probe
    // catches the same change when it happens between `ensure` and the call.
    let err = apply::apply(&cx, &st, Uuid::from_u128(1), HOST, 600, Some(&unverified()))
        .await
        .expect_err("probe");
    assert_eq!(err.token, Token::PostconditionFailed);
    assert_eq!(
        count(&w, "blocked4"),
        0,
        "the helper rolled the element back"
    );
}

#[tokio::test]
async fn layout_change_on_an_empty_table_recreates_it_and_on_a_used_table_drifts() {
    let w = World::new();
    cx!(w, cx);
    let hosts = state(&cx, facts(OPEN_HOSTS)).await;
    assert!(
        !w.nft.lock().expect("nft").tables["domovoi"]
            .sets
            .contains_key("blockednet4")
    );
    let _ = hosts;
    let prefixes = state(&cx, facts(OPEN)).await;
    assert_eq!(prefixes.ens.state.as_str(), "recreated");
    assert!(
        w.nft.lock().expect("nft").tables["domovoi"]
            .sets
            .contains_key("blockednet4")
    );
    // Used: a block exists, so the layout may not change underneath it.
    apply::apply(
        &cx,
        &prefixes,
        Uuid::from_u128(1),
        HOST,
        600,
        Some(&unverified()),
    )
    .await
    .expect("apply");
    let back = state(&cx, facts(OPEN_HOSTS)).await;
    assert!(back.ens.drift.is_some());
}

#[tokio::test]
async fn exclusion_sets_accept_coalesced_ranges() {
    let w = World::new();
    cx!(w, cx);
    let _ = state(&cx, facts(OPEN)).await;
    let nft = w.nft.lock().expect("nft");
    let excl4 = &nft.tables["domovoi"].sets["excl4"];
    assert!(
        excl4
            .elems
            .iter()
            .any(|e| e.lo == 0xC000_0200 && e.hi == 0xC000_02FF),
        "192.0.2.0/24"
    );
    let mut sorted: Vec<(u128, u128)> = excl4.elems.iter().map(|e| (e.lo, e.hi)).collect();
    sorted.sort_unstable();
    assert!(
        sorted.windows(2).all(|p| p[0].1 < p[1].0),
        "disjoint and ascending"
    );
    assert!(!nft.tables["domovoi"].sets["excl6"].elems.is_empty());
}

#[tokio::test]
async fn existing_flows_follow_the_flowtable_and_the_conntrack_deletion() {
    for (flowtable, conntrack_ok, want) in [
        (false, true, "n/a"),
        (true, true, "cut"),
        (true, false, "may_persist"),
    ] {
        let w = World::new();
        {
            let mut nft = w.nft.lock().expect("nft");
            nft.flowtable = flowtable;
            nft.conntrack_ok = conntrack_ok;
        }
        cx!(w, cx);
        let st = state(&cx, facts(OPEN)).await;
        let out = apply::apply(&cx, &st, Uuid::from_u128(1), HOST, 600, Some(&unverified()))
            .await
            .expect("apply");
        assert_eq!(out["existing_flows"], want);
        let _ = rollback::rollback(&cx, &st, Uuid::from_u128(1), HOST, 600).await;
    }
}

#[tokio::test]
async fn wide_prefixes_are_not_deleted_from_conntrack() {
    let w = World::new();
    w.nft.lock().expect("nft").flowtable = true;
    cx!(w, cx);
    let st = state(&cx, facts(OPEN)).await;
    let out = apply::apply(
        &cx,
        &st,
        Uuid::from_u128(1),
        "203.0.113.0/24",
        600,
        Some(&unverified()),
    )
    .await
    .expect("a /24 is the widest conntrack deletion");
    assert_eq!(out["existing_flows"], "cut");
    let calls = w.exec.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.program.ends_with("/conntrack") && c.args == ["-D", "-s", "203.0.113.0/24"])
    );
}

#[test]
fn bridge_gap_is_reported_for_a_bridged_physical_nic_fixture() {
    let root = tempfile::tempdir().expect("tempdir");
    let net = root.path().join("class/net");
    for dir in ["eth0", "br0/bridge", "eth0/device"] {
        std::fs::create_dir_all(net.join(dir)).expect("dir");
    }
    assert!(!bridge_gap(root.path()), "a NIC with no master is no gap");
    std::os::unix::fs::symlink(net.join("br0"), net.join("eth0/master")).expect("link");
    assert!(bridge_gap(root.path()));
    let plain = tempfile::tempdir().expect("tempdir");
    assert!(!bridge_gap(plain.path()));
}
