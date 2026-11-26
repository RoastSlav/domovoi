//! A hub that stops and starts again: what each stored state becomes.

use domovoi_core::capability::Availability;
use domovoi_core::install::codes;
use domovoi_core::ports::{HostReconciler, Presence};
use domovoi_core::{TargetStatus, ports::ActionAnswer};
use domovoi_testkit::build::T0;

use super::world::World;

const INSTALL: &str = "agent.package.install";

/// A queued target that waits for its host, with the first service stopped.
async fn parked(world: &World) -> domovoi_core::InstallTarget {
    world.set_availability(world.host, INSTALL, Availability::HostUnknown);
    let target = world.install("web-tools", "1.0.0").await;
    world.quiet().await;
    world.shutdown.cancel();
    world.quiet().await;
    target
}

#[tokio::test]
async fn restart_in_transferring_settles_not_run_hub_restarted() {
    let mut world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    let target = parked(&world).await;
    world
        .force(&target, |t| t.status = TargetStatus::Transferring)
        .await;
    world.restart();
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::HUB_RESTARTED);
    assert!(world.link.package_events().is_empty());
}

#[tokio::test]
async fn restart_in_queued_resumes() {
    let mut world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    let target = parked(&world).await;
    world.restart();
    world.quiet().await;
    assert_eq!(world.target(&target).await.status, TargetStatus::Queued);
    world.set_availability(world.host, INSTALL, Availability::Available);
    world.handle.wake();
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::Succeeded);
}

#[tokio::test]
async fn restart_in_dispatched_waits_for_the_agent() {
    let mut world = World::new().await;
    world.import("web-tools", "1.0.0", b"small archive").await;
    let target = parked(&world).await;
    let sent = world.dispatched(&target).await;
    world.link.set_presence(
        world.host,
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );
    world.link.clear_journal_epoch(world.host);
    world.restart();
    world.quiet().await;
    assert_eq!(world.target(&sent).await.status, TargetStatus::Dispatched);

    // The agent authenticates and its journal says the install finished.
    world.link.set_presence(
        world.host,
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    world
        .link
        .set_journal_epoch(world.host, World::epoch(world.host));
    world.link.answer(
        world.host,
        sent.execution,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    world.handle.reconcile_host(world.host).await;
    assert_eq!(world.target(&sent).await.status, TargetStatus::Succeeded);
}

#[tokio::test]
async fn failure_after_dispatch_commit_and_before_send_reconciles_to_not_run() {
    let mut world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    // The hub dies between the dispatch commit and the send: the task unwinds in the hook,
    // before anything is queued for the agent.
    world.link.on_start_capability(|_, _| {
        std::panic::resume_unwind(Box::new("the hub stopped"));
    });
    let target = world.install("web-tools", "1.0.0").await;
    let stuck = world.until(&target, TargetStatus::Dispatched).await;
    assert!(stuck.dispatched_epoch.is_some());
    world.quiet().await;
    assert!(
        world
            .link
            .package_events()
            .iter()
            .all(|e| !matches!(e, domovoi_testkit::link::PackageEvent::Capability { .. }))
    );
    world.link.on_start_capability(|_, _| {});
    world.restart();
    world.handle.reconcile_host(world.host).await;
    let done = world.settled(&target).await;
    assert_eq!(done.status, TargetStatus::NotRun);
    assert_eq!(done.failure.unwrap().code(), codes::NOT_RUN);
}
