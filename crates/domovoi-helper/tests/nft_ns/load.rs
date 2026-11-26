//! Tests that build the self-test's namespaces and send packets. They need `ip netns` (a mount
//! namespace with a writable `/run`) as well as a network namespace:
//!
//! ```text
//! unshare -Urnm sh -c 'mount -t tmpfs tmpfs /run && DOMOVOI_NS_TESTS=1 cargo test ...'
//! ```
//!
//! The ones that load the machine also need `DOMOVOI_LOAD_TESTS=1` and `DOMOVOI_BIN`, the path
//! of a built `domovoi` binary, which the self-test re-invokes as its UDP sender. They run in the
//! `containment-leak` job, never in the required checks.

use domovoi_core::containment::{Net, templates};
use domovoi_helper::nft::selftest::scenario::{Host, run_scenario, scenarios};
use domovoi_helper::nft::selftest::{self, current_key};

use super::support::{Ns, enabled, raw};

fn load_enabled(test: &str) -> bool {
    if !enabled(test) {
        return false;
    }
    if std::env::var("DOMOVOI_LOAD_TESTS").as_deref() == Ok("1")
        && std::env::var("DOMOVOI_BIN").is_ok()
    {
        return true;
    }
    eprintln!("SKIPPED {test}: set DOMOVOI_LOAD_TESTS=1 and DOMOVOI_BIN=<path of a built domovoi>");
    false
}

fn host<'a>(ns: &'a Ns, exe: &str, id: &str) -> Host<'a> {
    Host {
        exec: &ns.exec,
        inspect: &ns.inspect,
        clock: &ns.clock,
        exe: exe.to_owned(),
        id: id.to_owned(),
    }
}

/// A clock that moves with the wall, for the scenario caps. The fake clock never advances on
/// its own, so a scenario would run until its replacement minimum.
struct WallClock;

#[async_trait::async_trait]
impl domovoi_core::ports::Clock for WallClock {
    fn now(&self) -> jiff::Timestamp {
        jiff::Timestamp::now()
    }

    async fn sleep_until(&self, t: jiff::Timestamp) {
        if let Ok(wait) = std::time::Duration::try_from(jiff::Timestamp::now().duration_until(t)) {
            tokio::time::sleep(wait).await;
        }
    }
}

#[tokio::test]
async fn selftest_namespace_is_always_deleted() {
    if !enabled("selftest_namespace_is_always_deleted") {
        return;
    }
    let ns = Ns::new().await;
    let h = host(&ns, "/nonexistent/domovoi", "t1");
    h.build().await.expect("the namespaces are built");
    assert!(h.leftovers().await, "they exist before the teardown");
    h.teardown().await;
    assert!(!h.leftovers().await, "and are gone after it");
    // An error in the middle of a run still deletes them: the sender cannot start.
    let key = current_key("test", "test");
    let outcome = selftest::run(&h, key, None, true).await;
    assert!(outcome.is_err(), "a sender that cannot run is not a pass");
    assert!(
        !h.leftovers().await,
        "the namespaces are deleted on the error path too"
    );
}

#[tokio::test]
async fn selftest_never_touches_the_default_namespace_or_the_production_table() {
    if !enabled("selftest_never_touches_the_default_namespace_or_the_production_table") {
        return;
    }
    let ns = Ns::new().await;
    let (exit, _) = raw(&ns, "create table inet domovoi\nadd set inet domovoi blocked4 { type ipv4_addr; flags timeout; timeout 30s; size 5; }\n").await;
    assert_eq!(exit, Some(0));
    let before = listing(&ns).await;
    let h = host(&ns, "/nonexistent/domovoi", "t2");
    let _ = selftest::run(&h, current_key("test", "test"), None, true).await;
    let after = listing(&ns).await;
    assert_eq!(
        before, after,
        "the default namespace's ruleset is unchanged"
    );
    assert!(!after.contains("domovoi_selftest"));
}

async fn listing(ns: &Ns) -> String {
    use domovoi_core::ports::{ProcessExecutor, ProcessSpec};
    let spec = ProcessSpec {
        program: "/usr/sbin/nft".to_owned(),
        args: vec!["list".to_owned(), "ruleset".to_owned()],
        env: Default::default(),
        cwd: None,
        stdin: None,
        timeout: jiff::SignedDuration::from_secs(10),
        max_output_bytes: 1 << 20,
        run_as: None,
    };
    ns.exec.run(spec).await.expect("nft").stdout
}

#[tokio::test]
async fn leak_self_test_control_run_leaks_so_the_test_can_see_a_window() {
    if !load_enabled("leak_self_test_control_run_leaks_so_the_test_can_see_a_window") {
        return;
    }
    let bin = std::env::var("DOMOVOI_BIN").expect("DOMOVOI_BIN");
    let ns = Ns::new().await;
    let wall = WallClock;
    let h = Host {
        exec: &ns.exec,
        inspect: &ns.inspect,
        clock: &wall,
        exe: bin,
        id: "t3".to_owned(),
    };
    h.build().await.expect("build");
    let lone = scenarios(false)[0];
    let counts = run_scenario(&h, lone).await;
    h.teardown().await;
    let counts = counts.expect("the scenario ran");
    eprintln!(
        "RECORDED {}: leaked {} of {} sent over {} replacements; control leaked {} of {} ({:.3})",
        counts.name,
        counts.leaked,
        counts.sent,
        counts.replacements,
        counts.control_leaked,
        counts.control_sent,
        counts.control_ratio()
    );
    assert!(
        counts.control_ratio() > 0.01,
        "the split control must leak, or the test is blind"
    );
    let _ = (Net::parse("203.0.113.7/32"), templates::STEP1_TTL_S);
}
