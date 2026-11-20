//! The handles say whether their service is still running, which `/readyz` asks.

use crate::harness::{Setup, world};

#[tokio::test]
async fn handle_reports_not_alive_after_its_service_stops() {
    let world = world(Setup::default()).await;
    assert!(world.manager.is_alive(), "a running manager is alive");
    world.manager_task.abort();
    // Wait for the task to end so its queue closes.
    for _ in 0..50 {
        if !world.manager.is_alive() {
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(!world.manager.is_alive(), "a stopped manager is not alive");
}
