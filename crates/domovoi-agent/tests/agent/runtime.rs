//! The runtime wires everything together and stops promptly on cancellation.

use std::sync::Arc;

use domovoi_agent::{AgentDeps, AgentError, AgentRuntime};
use domovoi_testkit::{FakeProcessExecutor, ListRedactor};
use tokio_util::sync::CancellationToken;

use crate::common::{NoStats, install};
use crate::harness::{clock, empty_fs, identity};
use crate::hub::TestHub;
use crate::link::config;

fn deps(dir: &tempfile::TempDir) -> AgentDeps {
    AgentDeps {
        clock: clock(),
        executor: Arc::new(FakeProcessExecutor::new()),
        hostfs: empty_fs(dir),
        fs_stats: Arc::new(NoStats),
        container_source: None,
        redactor: Arc::new(ListRedactor::new(Vec::<String>::new())),
    }
}

#[tokio::test]
async fn runtime_refuses_to_start_before_enrollment() {
    let hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let result = AgentRuntime::new(config(&dir, &hub), deps(&dir)).await;
    assert!(matches!(result, Err(AgentError::NotEnrolled(_))));
}

#[tokio::test]
async fn runtime_shuts_down_within_two_seconds_on_cancel() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    identity.save(dir.path()).unwrap();
    install(&dir.path().join("skills"), "restart_app", "write");
    let runtime = AgentRuntime::new(config(&dir, &hub), deps(&dir))
        .await
        .unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(runtime.run(shutdown.clone()));
    let mut conn = hub.accept().await;
    let hello = conn.welcome(&identity, 10).await;
    let announced: Vec<&str> = hello.skills.iter().map(|s| s.name.as_str()).collect();
    // Skills load on Unix only; elsewhere the catalogue is empty.
    let expected: &[&str] = if cfg!(unix) { &["restart_app"] } else { &[] };
    assert_eq!(announced, expected);

    shutdown.cancel();
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(2), task).await;
    let result = stopped
        .expect("the runtime did not stop within 2 s")
        .unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert!(dir.path().join("checks.json").exists());
}
