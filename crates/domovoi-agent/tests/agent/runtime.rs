//! The runtime wires everything together and stops promptly on cancellation.

use std::sync::Arc;

use domovoi_agent::{AgentDeps, AgentError, AgentRuntime};
use domovoi_testkit::{FakeProcessExecutor, ListRedactor};
#[cfg(target_os = "linux")]
use tokio_util::sync::CancellationToken;

use crate::common::NoStats;
#[cfg(target_os = "linux")]
use crate::common::install;
#[cfg(target_os = "linux")]
use crate::harness::identity;
use crate::harness::{clock, empty_fs};
use crate::hub::TestHub;
use crate::link::config;

fn deps(dir: &tempfile::TempDir) -> AgentDeps {
    AgentDeps {
        clock: clock(),
        executor: Arc::new(FakeProcessExecutor::new()),
        hostfs: empty_fs(dir),
        toolfs: empty_fs(dir),
        fs_stats: Arc::new(NoStats),
        container_source: None,
        docker: None,
        redactor: Arc::new(ListRedactor::new(Vec::<String>::new())),
        logs: None,
    }
}

#[tokio::test]
async fn runtime_refuses_to_start_before_enrollment() {
    let hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let result = AgentRuntime::new(config(&dir, &hub), deps(&dir)).await;
    assert!(matches!(result, Err(AgentError::NotEnrolled(_))));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn runtime_shuts_down_within_two_seconds_on_cancel() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    identity.save(dir.path()).unwrap();
    let epoch = domovoi_agent::Journal::init(
        dir.path(),
        jiff::Timestamp::constant(1_700_000_000, 0),
        "test-boot",
        1,
    )
    .unwrap();
    install(&dir.path().join("skills"), "restart_app", "write");
    let runtime = AgentRuntime::new(config(&dir, &hub), deps(&dir))
        .await
        .unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(runtime.run(shutdown.clone()));
    let mut conn = hub.accept().await;
    let hello = conn.welcome(&identity, 10).await;
    assert_eq!(
        hello.journal_epoch, epoch,
        "a running agent reports its real journal epoch"
    );
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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn runtime_refuses_to_start_when_another_agent_owns_the_journal() {
    let hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    identity.save(dir.path()).unwrap();
    domovoi_agent::Journal::init(
        dir.path(),
        jiff::Timestamp::constant(1_700_000_000, 0),
        "test-boot",
        1,
    )
    .unwrap();
    // The first runtime owns the journal for as long as it lives.
    let held = domovoi_agent::Journal::open(
        dir.path(),
        "test-boot",
        1,
        domovoi_config::MutationLimits::default(),
    )
    .unwrap();
    let result = AgentRuntime::new(config(&dir, &hub), deps(&dir)).await;
    assert!(
        matches!(
            result,
            Err(AgentError::Journal(domovoi_agent::JournalError::Locked(_)))
        ),
        "a second agent must not start without the journal lock"
    );
    drop(held);

    let runtime = AgentRuntime::new(config(&dir, &hub), deps(&dir))
        .await
        .expect("the journal is free again");
    drop(runtime);
}
