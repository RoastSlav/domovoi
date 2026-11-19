use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use domovoi_config::MapEnv;
use domovoi_testkit::FakeClock;

use super::{Mode, Outcome, run};
use crate::runtime::HubDeps;

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

/// The clock stays put unless a test moves it: the stages complete on events, except the
/// investigation, whose effects wait for the clock ([`tick`]).
fn deps() -> HubDeps {
    deps_on(Arc::new(FakeClock::new(
        "2026-11-01T12:00:00Z".parse().unwrap(),
    )))
}

fn deps_on(clock: Arc<FakeClock>) -> HubDeps {
    HubDeps {
        clock,
        process_executor: None,
        metrics: None,
        apply: None,
        logs: None,
    }
}

/// Fails the test instead of hanging it.
async fn guard<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(30), future)
        .await
        .expect("selftest test timed out")
}

#[tokio::test]
async fn offline_selftest_passes_every_stage_without_touching_the_data_dir() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let clock = Arc::new(FakeClock::new("2026-11-01T12:00:00Z".parse().unwrap()));
    let ticker = {
        let clock = clock.clone();
        tokio::spawn(async move {
            loop {
                clock.advance(jiff::SignedDuration::from_millis(500));
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };
    let report = guard(run(config(""), deps_on(clock), Mode::Offline)).await;
    ticker.abort();
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
    let report = guard(run(config(""), deps(), Mode::Online)).await;
    let triage = report.stages.iter().find(|s| s.stage == "triage").unwrap();
    assert!(matches!(triage.outcome, Outcome::Skip(_)), "{report}");
    let card = report.stages.iter().find(|s| s.stage == "card").unwrap();
    assert!(matches!(card.outcome, Outcome::Fail(_)), "{report}");
    let gateway = report.stages.iter().find(|s| s.stage == "gateway").unwrap();
    assert!(matches!(gateway.outcome, Outcome::Skip(_)), "{report}");
    assert!(!report.passed());
}
