//! Settling dispatched targets from the agent's journal.

use domovoi_core::install::codes;
use domovoi_core::ports::{ActionAnswer, HostReconciler, UnknownReason};
use domovoi_core::{InstallTarget, JournalEpoch, TargetStatus};
use uuid::Uuid;

use super::world::World;

/// A dispatched target whose agent is connected and has not been asked yet.
async fn sent(world: &World) -> InstallTarget {
    world.import("web-tools", "1.0.0", b"small archive").await;
    world.set_availability(
        world.host,
        "agent.package.install",
        domovoi_core::capability::Availability::HostUnknown,
    );
    let target = world.install("web-tools", "1.0.0").await;
    world.quiet().await;
    world.dispatched(&target).await
}

async fn reconciled(world: &World, target: &InstallTarget, answer: ActionAnswer) -> InstallTarget {
    world
        .link
        .answer(world.host, target.execution.as_action(), answer);
    world.handle.reconcile_host(world.host).await;
    world.target(target).await
}

#[tokio::test]
async fn reconcile_finished_ok_marks_succeeded_unconfirmed() {
    let world = World::new().await;
    let target = sent(&world).await;
    let done = reconciled(
        &world,
        &target,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    )
    .await;
    assert_eq!(done.status, TargetStatus::Succeeded);
    assert!(!done.outputs_confirmed);
    assert!(done.outputs.is_none());
    assert_eq!(
        world
            .audit("package.install_confirmed_by_journal")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn reconcile_finished_failed_parses_the_code() {
    let world = World::new().await;
    let target = sent(&world).await;
    let done = reconciled(
        &world,
        &target,
        ActionAnswer::Finished {
            succeeded: false,
            detail: Some("bad_signature: no match".to_owned()),
        },
    )
    .await;
    assert_eq!(done.status, TargetStatus::Failed);
    assert_eq!(done.failure.unwrap().code(), "bad_signature");

    let world = World::new().await;
    let target = sent(&world).await;
    let done = reconciled(
        &world,
        &target,
        ActionAnswer::Finished {
            succeeded: false,
            detail: Some("garbled".to_owned()),
        },
    )
    .await;
    assert_eq!(done.failure.unwrap().code(), codes::AGENT_REFUSED);
}

#[tokio::test]
async fn reconcile_not_found_is_not_run() {
    let world = World::new().await;
    let target = sent(&world).await;
    let done = reconciled(&world, &target, ActionAnswer::NotFound).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::NOT_RUN);
}

#[tokio::test]
async fn reconcile_unknown_reasons_map_to_outcome_unknown() {
    for (reason, code) in [
        (UnknownReason::NoStartRecord, "no_start_record"),
        (UnknownReason::AgentRestarted, "agent_restarted"),
        (UnknownReason::JournalLost, "journal_lost"),
    ] {
        let world = World::new().await;
        let target = sent(&world).await;
        let done = reconciled(&world, &target, ActionAnswer::Unknown { reason }).await;
        assert_eq!(done.status, TargetStatus::OutcomeUnknown, "{code}");
        assert_eq!(done.failure.unwrap().code(), code);
    }
}

#[tokio::test]
async fn reconcile_with_changed_epoch_is_outcome_unknown() {
    let world = World::new().await;
    let target = sent(&world).await;
    world
        .link
        .set_journal_epoch(world.host, JournalEpoch::from_uuid(Uuid::from_u128(0xfeed)));
    world.handle.reconcile_host(world.host).await;
    let done = world.target(&target).await;
    assert_eq!(done.status, TargetStatus::OutcomeUnknown);
    assert_eq!(done.failure.unwrap().code(), codes::EPOCH_CHANGED);

    let world = World::new().await;
    let target = sent(&world).await;
    world.force(&target, |t| t.dispatched_epoch = None).await;
    world.handle.reconcile_host(world.host).await;
    let done = world.target(&target).await;
    assert_eq!(done.failure.unwrap().code(), codes::EPOCH_UNKNOWN);
}

#[tokio::test]
async fn reconcile_running_stays_dispatched() {
    let world = World::new().await;
    let target = sent(&world).await;
    let still = reconciled(&world, &target, ActionAnswer::Running).await;
    assert_eq!(still.status, TargetStatus::Dispatched);
    assert!(still.failure.is_none());
}
