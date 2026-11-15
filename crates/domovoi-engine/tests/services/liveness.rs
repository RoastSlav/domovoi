//! The handles say whether their service is still running, which `/readyz` asks.

use domovoi_engine::ActionSettings;

use crate::actions_harness::{actions, seeded};

#[tokio::test]
async fn handle_reports_not_alive_after_its_service_stops() {
    let world = seeded().await;
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

#[tokio::test]
async fn action_handle_reports_not_alive_after_its_service_stops() {
    let running = actions(ActionSettings::default()).await;
    assert!(
        running.service.is_alive(),
        "a running action service is alive"
    );
    running.action_task.abort();
    for _ in 0..50 {
        if !running.service.is_alive() {
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(
        !running.service.is_alive(),
        "a stopped action service is not alive"
    );
}
