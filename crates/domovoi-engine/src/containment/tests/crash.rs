//! The containment driver crashed at every commit of one block's history. Each commit is atomic,
//! so a crash leaves either all of its change, journal row and audit row or none of them; the
//! driver never sends a frame for a phase it has not committed, and a block left behind settles
//! from the agent's journal, never by a second apply.

use domovoi_core::ExecutionId;
use domovoi_core::attest::Authority;
use domovoi_core::ports::{BlockPhase, HostReconciler};
use domovoi_testkit::crash::{CrashPlan, CrashPoint};

use super::world::{World, ok};
use crate::containment::phases::execute_containment;

#[tokio::test]
async fn a_block_survives_a_crash_at_every_commit_and_is_never_applied_twice() {
    // The commits of a happy block: create (applying), applied, confirming, confirmed.
    for at_commit in 1..=4 {
        for point in [CrashPoint::BeforeCommit, CrashPoint::AfterCommit] {
            let (world, crashing) = World::crashing().await;
            world.verify();
            world.relay.queue_result(ok(&[]));
            world.relay.queue_result(ok(&[]));
            crashing.arm(CrashPlan { at_commit, point });
            let (authorized, prepared, session, epoch) =
                world.step("198.51.100.0/24", 600, Authority::Human).await;
            let id: ExecutionId = authorized.execution().execution;
            execute_containment(world.driver.deps(), &authorized, &prepared, session, epoch).await;
            assert!(
                crashing.crashed(),
                "commit {at_commit}, {point:?}: the crash never happened"
            );
            crashing.restart();

            let stored = world.store_block(id).await;
            match (at_commit, point, &stored) {
                // Nothing was committed, so nothing was sent.
                (1, CrashPoint::BeforeCommit, None) => {
                    assert!(world.relay.dispatches().is_empty());
                }
                (1, CrashPoint::BeforeCommit, Some(_)) => panic!("a block with no commit"),
                (_, _, None) => panic!("commit {at_commit}, {point:?}: the block vanished"),
                (_, _, Some(row)) => {
                    // The stored phase is a whole one the driver passes through.
                    assert!(matches!(
                        row.phase,
                        BlockPhase::Applying
                            | BlockPhase::Applied
                            | BlockPhase::Confirming
                            | BlockPhase::Confirmed
                            | BlockPhase::NotRun
                            | BlockPhase::Expired
                            | BlockPhase::RolledBack
                            | BlockPhase::Failed
                    ));
                }
            }
            // A restart reconciles from the journal and re-sends nothing.
            let before = world.relay.dispatches().len();
            world.driver.reconcile_host(world.host).await;
            assert_eq!(world.relay.dispatches().len(), before);
            let applies = world
                .relay
                .events()
                .iter()
                .filter(|event| *event == "dispatch:apply")
                .count();
            assert!(applies <= 1, "commit {at_commit}, {point:?}: applied twice");
        }
    }
}

#[tokio::test]
async fn crash_between_applying_commit_and_send_leaves_not_run() {
    use domovoi_core::ports::ActionAnswer;
    let (world, crashing) = World::crashing().await;
    world.verify();
    // The process dies right after the `Applying` row is committed, before the frame goes out.
    crashing.arm(CrashPlan {
        at_commit: 1,
        point: CrashPoint::AfterCommit,
    });
    let (authorized, prepared, session, epoch) =
        world.step("198.51.100.0/24", 600, Authority::Human).await;
    let id: ExecutionId = authorized.execution().execution;
    execute_containment(world.driver.deps(), &authorized, &prepared, session, epoch).await;
    assert!(crashing.crashed());
    crashing.restart();
    assert_eq!(world.block(id).await.phase, BlockPhase::Applying);
    assert!(world.relay.dispatches().is_empty(), "nothing was sent");

    // On the next hello the agent's journal has never heard of the execution: the phase did not
    // run, so the block is not run, and nothing is sent to make up for it.
    world
        .link
        .answer(world.host, id.as_action(), ActionAnswer::NotFound);
    world.driver.reconcile_host(world.host).await;
    assert_eq!(world.block(id).await.phase, BlockPhase::NotRun);
    assert!(world.relay.dispatches().is_empty());
}
