//! The gates the sweep itself is held to: every kind of write, every commit site, and
//! snapshots that tell different states apart.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use domovoi_core::ports::IncidentRepo;
use domovoi_core::{IncidentEvent, IncidentPolicy, UserId, apply};
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::crash::{Snapshot, TxSummary};

use crate::scenarios::{action_lifecycle, incident_lifecycle};
use crate::sweep::BoxFuture;
use crate::world::{World, host_id};

/// A scenario bound to the in-memory store.
type Run = fn(&mut World<InMemoryStore>) -> BoxFuture<'_, ()>;

/// The files a commit goes through. A later writer that commits anywhere else fails
/// [`commit_sites_are_the_ones_the_sweep_exercises`] until it adds a scenario here.
const COMMIT_SITES: [&str; 2] = ["actions/mod.rs", "manager/actor.rs"];

/// Every audit kind the action writer can produce, including the chat-token rotation. A scenario
/// has to exercise each one, and a new producer has to be added here.
const AUDIT_KINDS: [&str; 12] = [
    "action.proposed",
    "action.approved",
    "action.denied",
    "action.expired",
    "action.rejected",
    "action.blocked",
    "action.dispatched",
    "action.succeeded",
    "action.failed",
    "action.verified",
    "action.verify_failed",
    "action.token_rotated",
];

/// Runs a scenario once with the wrapper recording, and returns what it committed.
async fn recorded(run: Run) -> Vec<TxSummary> {
    let mut world = World::new(Arc::new(InMemoryStore::new())).await;
    world.crashing.record_only();
    (run)(&mut world).await;
    let summaries = world.crashing.summaries();
    assert!(!summaries.is_empty(), "the scenario committed nothing");
    world.shutdown().await;
    summaries
}

/// Every write kind, action status, journal kind and audit kind the writers produce is
/// exercised by at least one commit of one scenario.
#[tokio::test]
async fn sweep_covers_every_kind_of_write() {
    let mut writes = BTreeSet::new();
    let mut journal = BTreeSet::new();
    let mut audit = BTreeSet::new();
    for run in [
        incident_lifecycle::<InMemoryStore>,
        action_lifecycle::<InMemoryStore>,
    ] {
        for summary in recorded(run).await {
            writes.extend(summary.writes);
            journal.extend(summary.journal);
            audit.extend(summary.audit);
        }
    }

    for required in [
        "signal",
        "incident:new",
        "incident:version",
        "incident_event",
        "incident:discard",
        "action:new",
    ] {
        assert!(
            writes.contains(required),
            "no scenario writes {required}, so the sweep does not cover every write\nfound: {writes:#?}"
        );
    }
    for status in [
        "proposed",
        "approved",
        "denied",
        "expired",
        "rejected",
        "dispatched",
        "succeeded",
        "failed",
        "verified",
        "verify_failed",
    ] {
        let wanted = format!("action:status:{status}");
        assert!(
            writes.contains(&wanted),
            "no scenario moves an action to {status}, so the sweep does not cover every status\nfound: {writes:#?}"
        );
    }
    assert!(
        journal.contains("incident.timeline"),
        "no scenario writes a mirrored timeline journal row\nfound: {journal:#?}"
    );
    assert!(
        journal.contains("action.transition"),
        "no scenario writes an action transition journal row\nfound: {journal:#?}"
    );

    // The audit contract, both ways: every kind the action writer produces must be exercised,
    // and a kind this list does not know is a new producer that needs reviewing here.
    let observed: BTreeSet<&str> = audit.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = AUDIT_KINDS.into_iter().collect();
    let missing: Vec<&str> = expected.difference(&observed).copied().collect();
    assert!(
        missing.is_empty(),
        "no scenario produces these audit kinds, so the sweep does not cover them: {missing:?}"
    );
    let unexpected: Vec<&str> = observed.difference(&expected).copied().collect();
    assert!(
        unexpected.is_empty(),
        "the writers now produce audit kinds the contract does not list: {unexpected:?}; review \
         the new producer and extend AUDIT_KINDS in the same change"
    );
}

/// Every unit of work commit in the engine is one the sweep drives. A new commit site means
/// a scenario is missing: the sweep no longer proves that write is crash-safe.
#[test]
fn commit_sites_are_the_ones_the_sweep_exercises() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeSet::new();
    scan(&root, &root, &mut found);
    let expected: BTreeSet<String> = COMMIT_SITES.iter().map(|site| (*site).to_owned()).collect();
    assert_eq!(
        found, expected,
        "the engine commits from a file this sweep does not expect: add a scenario that \
         exercises the new write and add its file to COMMIT_SITES, or the crash guarantee is \
         no longer tested for it"
    );
}

/// Collects the paths under `dir` whose code calls `.commit(`, skipping comment lines.
fn scan(root: &Path, dir: &Path, found: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(root, &path, found);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let commits = source.lines().any(|line| {
            let code = line.trim_start();
            !code.starts_with("//") && code.contains(".commit(")
        });
        if commits && let Ok(relative) = path.strip_prefix(root) {
            found.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Two states that differ in one status are not equal.
#[tokio::test]
async fn snapshots_distinguish_states() {
    let store = Arc::new(InMemoryStore::new());
    let incident =
        open_incident(host_id(), "docker.container", Some("nginx")).expect("a valid incident");
    IncidentRepo::insert(&*store, &incident)
        .await
        .expect("the incident is stored");
    let before = Snapshot::capture(&*store)
        .await
        .expect("the state is readable");

    let mut acked = incident.clone();
    apply(
        &mut acked,
        IncidentEvent::Ack { by: UserId::new() },
        &IncidentPolicy::default(),
        T0,
    )
    .expect("an open incident can be acked");
    IncidentRepo::update(&*store, &acked)
        .await
        .expect("the incident is stored again");
    let after = Snapshot::capture(&*store)
        .await
        .expect("the state is readable");

    assert_ne!(before, after, "a status change must show in the snapshot");
}

/// The same scenario over the same store makes the same commits twice: the sweep compares a
/// crash run with the dry run, so a scenario that varies would make that comparison useless.
#[tokio::test]
async fn scenarios_are_deterministic() {
    for run in [
        incident_lifecycle::<InMemoryStore>,
        action_lifecycle::<InMemoryStore>,
    ] {
        let first = recorded(run).await;
        let second = recorded(run).await;
        assert_eq!(
            first, second,
            "the same scenario committed different things twice"
        );
    }
}
