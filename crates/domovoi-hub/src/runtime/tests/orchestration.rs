use jiff::Timestamp;

use super::harness::Harness;

fn start() -> Timestamp {
    "2026-11-19T12:00:00Z".parse().unwrap()
}

#[tokio::test]
async fn flag_off_starts_no_operation_service() {
    let harness = Harness::new("", start()).await;

    assert!(harness.runtime.as_ref().unwrap().operations_task.is_none());

    harness.stop().await;
}

#[tokio::test]
async fn flag_on_starts_the_operation_service_and_the_composite_reconciler() {
    let harness = Harness::new(
        "[orchestration]
enabled = true",
        start(),
    )
    .await;

    // The composite itself is covered in `orchestration.rs`; here the service is running
    // and stops with the hub, which `stop` proves by joining its task.
    assert!(harness.runtime.as_ref().unwrap().operations_task.is_some());

    harness.stop().await;
}
