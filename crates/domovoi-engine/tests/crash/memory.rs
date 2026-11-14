//! The sweep over the in-memory store: a restart needs no reopening, because the store
//! itself survives.

use std::sync::Arc;

use domovoi_testkit::InMemoryStore;
use domovoi_testkit::crash::InMemoryReopen;

use crate::scenarios::{action_lifecycle, incident_lifecycle};
use crate::sweep::{BoxFuture, Scenario, sweep};

/// A fresh store per case.
fn make() -> BoxFuture<'static, Arc<InMemoryStore>> {
    Box::pin(async { Arc::new(InMemoryStore::new()) })
}

#[tokio::test]
async fn incident_lifecycle_survives_a_crash_at_every_commit_in_memory() {
    let scenario = Scenario {
        name: "incident_lifecycle",
        run: incident_lifecycle::<InMemoryStore>,
        min_commits: 14,
    };
    sweep(&scenario, &make, &InMemoryReopen).await;
}

#[tokio::test]
async fn action_lifecycle_survives_a_crash_at_every_commit_in_memory() {
    let scenario = Scenario {
        name: "action_lifecycle",
        run: action_lifecycle::<InMemoryStore>,
        min_commits: 24,
    };
    sweep(&scenario, &make, &InMemoryReopen).await;
}
