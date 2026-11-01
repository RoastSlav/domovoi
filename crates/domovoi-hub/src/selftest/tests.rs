use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use domovoi_config::MapEnv;
use domovoi_core::ports::Clock;
use jiff::Timestamp;

use super::{Mode, Outcome, run};
use crate::runtime::HubDeps;

/// Delivery and triage wait on real time here, as in the binary.
struct WallClock;

#[async_trait]
impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }

    async fn sleep_until(&self, t: Timestamp) {
        if let Ok(wait) = Duration::try_from(Timestamp::now().duration_until(t)) {
            tokio::time::sleep(wait).await;
        }
    }
}

fn config(extra: &str) -> domovoi_config::HubConfig {
    let text = format!(
        r#"
[hub]
name = "selftest-hub"
data_dir = "/nonexistent/never-used"

[web]
bind = "127.0.0.1:0"

[agent_server]
bind = "127.0.0.1:0"

{extra}
"#
    );
    domovoi_config::parse(&text, &MapEnv::new()).unwrap().0
}

fn deps() -> HubDeps {
    HubDeps {
        clock: Arc::new(WallClock),
        process_executor: None,
        metrics: None,
    }
}

#[tokio::test]
async fn offline_selftest_passes_every_stage_without_touching_the_data_dir() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let report = tokio::time::timeout(
        Duration::from_secs(60),
        run(config(""), deps(), Mode::Offline),
    )
    .await
    .unwrap();
    let stages: Vec<_> = report.stages.iter().map(|s| s.stage).collect();
    assert_eq!(
        stages,
        ["config", "store", "manager", "triage", "card", "gateway"]
    );
    assert!(
        report.stages.iter().all(|s| s.outcome == Outcome::Pass),
        "{report}"
    );
    assert!(report.passed());
    assert!(!std::path::Path::new("/nonexistent/never-used").exists());
}

#[tokio::test]
async fn online_selftest_without_a_gateway_fails_at_the_card_stage() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let report = tokio::time::timeout(
        Duration::from_secs(60),
        run(config(""), deps(), Mode::Online),
    )
    .await
    .unwrap();
    let triage = report.stages.iter().find(|s| s.stage == "triage").unwrap();
    assert!(matches!(triage.outcome, Outcome::Skip(_)), "{report}");
    let card = report.stages.iter().find(|s| s.stage == "card").unwrap();
    assert!(matches!(card.outcome, Outcome::Fail(_)), "{report}");
    assert!(!report.passed());
}
