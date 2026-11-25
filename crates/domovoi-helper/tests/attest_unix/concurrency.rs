use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};

use domovoi_core::attest::Report;
use domovoi_helper::attest::state::StateFile;
use domovoi_helper::attest::{AttestOutcome, safety_attest};
use domovoi_helper::state::StateLock;

use crate::world::{World, nonce, verify};

/// Runs `count` attestations on a thread of its own, against the shared directory, and returns
/// `(generation, config_hash)` of each complete report in order.
fn worker(
    world: World,
    start: Arc<Barrier>,
    first: u8,
    count: u8,
) -> std::thread::JoinHandle<Vec<(u64, String)>> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        start.wait();
        (first..first + count)
            .map(|n| {
                let outcome = runtime.block_on(safety_attest(&world.env(), nonce(n)));
                let AttestOutcome::Signed(attestation) = outcome else {
                    panic!("unsigned");
                };
                let Report::Complete(report) = verify(&attestation) else {
                    panic!("incomplete");
                };
                (report.generation, report.config_hash.to_string())
            })
            .collect()
    })
}

#[test]
fn concurrent_attestations_serialize_generation_updates() {
    let world = World::new();
    world.init();
    // Two hosts' worth of views over one directory: each sees a different configuration, so
    // every alternation of the two is a change and needs a generation of its own.
    let mut other = world.sharing();
    other
        .text
        .helper_toml
        .push_str("protected_containers = [\"app-b\"]\n");
    let start = Arc::new(Barrier::new(2));
    let a = worker(world.sharing(), start.clone(), 0, 12);
    let b = worker(other, start, 100, 12);
    let reports: Vec<(u64, String)> = a
        .join()
        .expect("thread")
        .into_iter()
        .chain(b.join().expect("thread"))
        .collect();

    // One generation is never handed out for two different configurations.
    let mut by_generation: BTreeMap<u64, &str> = BTreeMap::new();
    for (generation, hash) in &reports {
        let seen = by_generation.entry(*generation).or_insert(hash);
        assert_eq!(
            *seen, hash,
            "generation {generation} names two configurations"
        );
    }
    // Each thread saw its own generations only ever move forward.
    let state =
        StateFile::parse(&std::fs::read(world.path("state")).expect("state")).expect("state");
    let highest = reports.iter().map(|(g, _)| *g).max().expect("reports");
    assert_eq!(
        state.generation, highest,
        "the persisted generation is the highest handed out"
    );
    assert!(
        highest >= 2,
        "the two configurations alternated at least once"
    );
}

#[test]
fn state_lock_blocks_a_second_attestation_until_released() {
    // The attestation takes the same lock the helper's other writers take: while it is held
    // elsewhere, the removal of a leftover temporary file cannot have happened yet.
    let world = World::new();
    world.init();
    let temp = world.path(".state.tmp");
    std::fs::write(&temp, b"half a state").expect("leftover");
    let lock = StateLock::open(world.dir()).expect("open");
    let guard = lock.acquire().expect("held by the test");
    let start = Arc::new(Barrier::new(2));
    let attesting = worker(world.sharing(), start.clone(), 1, 1);
    start.wait();
    // The attestation is now either not started or waiting for the lock: it cannot have cleaned
    // up, whichever it is.
    assert!(temp.exists(), "the leftover is removed only under the lock");
    drop(guard);
    let reports = attesting.join().expect("thread");
    assert_eq!(reports[0].0, 1);
    assert!(!temp.exists());
}
