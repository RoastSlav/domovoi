//! The sweep: run a scenario once to learn its commit sequence, then once per commit and
//! crash point, restart, and check that durable state is exactly the committed prefix, that
//! the invariants hold before and after recovery, and that the services work again.
//!
//! The comparison is against snapshots the dry run captured after every commit, so it is
//! exact: a crash before commit `k` must leave the state of `k - 1` commits, a crash after
//! it the state of `k`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use domovoi_testkit::crash::{
    CrashPlan, CrashPoint, Reopen, Snapshot, SweepStore, check_invariants,
};

use super::world::World;

/// A boxed future, so a scenario can be a plain function shared by two test binaries.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Waits until the wrapper holds one snapshot per counted commit, twice in a row: no commit
/// is still applying or capturing.
async fn idle<S: SweepStore>(world: &World<S>) {
    let mut before = u64::MAX;
    for _ in 0..1_000 {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        let commits = world.crashing.commits();
        if world.crashing.snapshots().len() as u64 == commits + 1 && commits == before {
            return;
        }
        before = commits;
    }
    panic!("the dry run never stopped committing");
}

/// One lifecycle the sweep drives over a store.
pub struct Scenario<S: SweepStore> {
    /// What the scenario is called, in every failure message.
    pub name: &'static str,
    /// Runs every step against a fresh world and returns when quiescent.
    pub run: fn(&mut World<S>) -> BoxFuture<'_, ()>,
    /// The fewest commits the scenario must make, so one that silently does nothing fails.
    pub min_commits: u64,
}

/// Runs `scenario` fault-free, then once per commit and crash point, restarting after each
/// crash and proving the system recovers.
pub async fn sweep<S: SweepStore>(
    scenario: &Scenario<S>,
    make: &dyn Fn() -> BoxFuture<'static, Arc<S>>,
    reopen: &dyn Reopen<S>,
) {
    // The dry run: every commit recorded, and one snapshot of the inner store per commit.
    let mut world = World::new(make().await).await;
    world.crashing.record_only();
    world
        .crashing
        .record_snapshots()
        .await
        .expect("the dry run records the state before its first commit");
    (scenario.run)(&mut world).await;
    // The executor's last transition and a run's trailing verdict write are detached, so
    // wait until nothing is committing before reading the sequence: a commit counted but
    // not yet snapshotted would shift every later index.
    world.settle().await;
    idle(&world).await;
    let snapshots = world.crashing.snapshots();
    let dry = world.crashing.summaries();
    let commits = world.crashing.commits();
    world.shutdown().await;

    assert!(
        commits >= scenario.min_commits,
        "scenario {} made {commits} commits, fewer than the {} it must exercise",
        scenario.name,
        scenario.min_commits,
    );
    assert_eq!(
        snapshots.len() as u64,
        commits + 1,
        "scenario {}: one snapshot per commit, plus the state before the first",
        scenario.name,
    );

    for at in 1..=commits {
        for point in [CrashPoint::BeforeCommit, CrashPoint::AfterCommit] {
            let mut world = World::new(make().await).await;
            world.crashing.arm(CrashPlan {
                at_commit: at,
                point,
            });
            (scenario.run)(&mut world).await;
            assert!(
                world.crashing.crashed(),
                "scenario {}, commit {at}, {point:?}: the planned crash never happened",
                scenario.name,
            );

            let attempted = world.crashing.summaries();
            let up_to = at as usize;
            assert_eq!(
                attempted.get(..up_to),
                Some(&dry[..up_to]),
                "scenario {}, commit {at}, {point:?}: the re-run diverged from the dry run\n\
                 attempted: {attempted:#?}\ndry run: {dry:#?}",
                scenario.name,
            );

            let surviving = Snapshot::capture(&*world.store)
                .await
                .expect("the surviving state is readable");
            let expected = match point {
                CrashPoint::BeforeCommit => snapshots[(at - 1) as usize].clone(),
                CrashPoint::AfterCommit => snapshots[at as usize].clone(),
            };
            assert_eq!(
                surviving, expected,
                "scenario {}, commit {at}, {point:?}: the surviving state is not the committed prefix\n\
                 found: {surviving:#?}\nexpected: {expected:#?}",
                scenario.name,
            );

            let store = world.crashing.inner();
            world.shutdown().await;
            let reopened = reopen.reopen(store).await;

            if let Err(violations) = check_invariants(&*reopened).await {
                panic!(
                    "scenario {}, commit {at}, {point:?}: invariants after the crash: {violations:#?}",
                    scenario.name,
                );
            }

            let restarted = World::restarted(reopened.clone());
            if let Err(err) = restarted.recover().await {
                panic!(
                    "scenario {}, commit {at}, {point:?}: the restart did not recover: {err}",
                    scenario.name,
                );
            }
            if let Err(violations) = check_invariants(&*reopened).await {
                panic!(
                    "scenario {}, commit {at}, {point:?}: invariants after recovery: {violations:#?}",
                    scenario.name,
                );
            }
            restarted.assert_services_work().await;
            restarted.shutdown().await;
        }
    }
}
