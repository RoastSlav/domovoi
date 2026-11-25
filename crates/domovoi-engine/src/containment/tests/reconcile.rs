//! Settling blocks from the agent's journal when the host's agent authenticates.

use domovoi_core::ExecutionId;
use domovoi_core::attest::Authority;
use domovoi_core::ports::{ActionAnswer, BlockPhase, HostReconciler};
use jiff::SignedDuration;

use super::world::World;
use crate::containment::phases::execute_containment;

/// Runs a block until its first phase has no usable answer, so it stays `Applying`.
async fn stuck_applying(world: &World) -> ExecutionId {
    let (authorized, prepared, session, epoch) =
        world.step("198.51.100.0/24", 600, Authority::Human).await;
    let id = authorized.execution().execution;
    execute_containment(world.driver.deps(), &authorized, &prepared, session, epoch).await;
    assert_eq!(world.block(id).await.phase, BlockPhase::Applying);
    id
}

#[tokio::test]
async fn reconcile_maps_each_journal_answer_per_phase() {
    for (answer, expected) in [
        (
            ActionAnswer::Finished {
                succeeded: true,
                detail: None,
            },
            BlockPhase::Applied,
        ),
        (
            ActionAnswer::Finished {
                succeeded: false,
                detail: Some("table_drift: size differs".to_owned()),
            },
            BlockPhase::Failed,
        ),
        (ActionAnswer::NotFound, BlockPhase::NotRun),
    ] {
        let world = World::new().await;
        world.verify();
        let id = stuck_applying(&world).await;
        world.link.answer(world.host, id.as_action(), answer);
        world.driver.reconcile_host(world.host).await;
        assert_eq!(world.block(id).await.phase, expected);
    }
}

#[tokio::test]
async fn a_running_phase_is_asked_about_again_later() {
    let world = World::new().await;
    world.verify();
    let id = stuck_applying(&world).await;
    world
        .link
        .answer(world.host, id.as_action(), ActionAnswer::Running);
    world.driver.reconcile_host(world.host).await;
    assert_eq!(world.block(id).await.phase, BlockPhase::Applying);
}

#[tokio::test]
async fn applied_past_30s_without_confirm_settles_expired() {
    let world = World::new().await;
    world.verify();
    let id = stuck_applying(&world).await;
    world.link.answer(
        world.host,
        id.as_action(),
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    world.driver.reconcile_host(world.host).await;
    assert_eq!(world.block(id).await.phase, BlockPhase::Applied);
    // Inside the kernel's window an applied block waits; past it, the kernel has removed it.
    world.driver.reconcile_host(world.host).await;
    assert_eq!(world.block(id).await.phase, BlockPhase::Applied);
    world.clock.advance(SignedDuration::from_secs(31));
    world.driver.reconcile_host(world.host).await;
    assert_eq!(world.block(id).await.phase, BlockPhase::Expired);
}

#[tokio::test]
async fn a_hub_restart_in_the_window_retries_nothing() {
    let world = World::new().await;
    world.verify();
    stuck_applying(&world).await;
    let before = world.relay.dispatches().len();
    world.driver.reconcile_host(world.host).await;
    assert_eq!(
        world.relay.dispatches().len(),
        before,
        "reconcile only reads"
    );
}
