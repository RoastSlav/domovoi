//! The drill suite of `domovoi helper containment-drill`, run once in throwaway namespaces and
//! asserted under each drill's own name, plus the three named tests of the containment milestone
//! that the same drills answer. They run only with `DOMOVOI_NS_TESTS=1` inside `unshare -Urnm`
//! with a writable `/run`, and the expiry drill waits out the kernel's 30 s timeout.

use std::collections::BTreeMap;

use domovoi_helper::nft::drill;
use domovoi_helper::nft::selftest::scenario::Host;
use tokio::sync::OnceCell;

use super::support::{Ns, enabled};

/// A clock that moves with the wall: the expiry drill waits for the kernel's own timer.
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

static RESULTS: OnceCell<BTreeMap<String, String>> = OnceCell::const_new();

/// Runs every drill once for the whole binary and returns each drill's result text.
async fn results() -> &'static BTreeMap<String, String> {
    RESULTS
        .get_or_init(|| async {
            let ns = Ns::new().await;
            let wall = WallClock;
            let host = Host {
                exec: &ns.exec,
                inspect: &ns.inspect,
                clock: &wall,
                exe: "/nonexistent/domovoi".to_owned(),
                id: format!("dr{}", std::process::id()),
            };
            host.build().await.expect("the drill namespaces are built");
            let out = drill::run(&host, ns.dir.path()).await;
            host.teardown().await;
            out.expect("the drills ran")
        })
        .await
}

async fn passes(drill_name: &str) {
    let got = results().await.get(drill_name).cloned();
    assert_eq!(got.as_deref(), Some("ok"), "{drill_name}");
}

macro_rules! drill_test {
    ($name:ident, $drill:literal) => {
        #[tokio::test]
        async fn $name() {
            if !enabled(stringify!($name)) {
                return;
            }
            passes($drill).await;
        }
    };
}

drill_test!(
    drill_expiry_on_time_with_agent_stopped,
    "drill_expiry_on_time_with_agent_stopped"
);
drill_test!(
    drill_excluded_address_refused,
    "drill_excluded_address_refused"
);
drill_test!(
    drill_hub_unreachable_during_confirmation_window_block_gone_within_30s,
    "drill_hub_unreachable_during_confirmation_window_block_gone_within_30s"
);
drill_test!(drill_agent_restart_mid_ttl, "drill_agent_restart_mid_ttl");
drill_test!(
    drill_duplicate_dispatch_is_idempotent,
    "drill_duplicate_dispatch_is_idempotent"
);
drill_test!(
    drill_max_active_reached_is_refused,
    "drill_max_active_reached_is_refused"
);
// The milestone's own names: the same behaviour, answered by the same drills.
drill_test!(
    block_expires_with_agent_stopped,
    "drill_expiry_on_time_with_agent_stopped"
);
drill_test!(
    unconfirmed_block_expires_in_30s,
    "drill_expiry_on_time_with_agent_stopped"
);
drill_test!(excluded_address_refused, "drill_excluded_address_refused");
