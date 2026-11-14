//! The crash sweep over real SQLite: a restart closes the pools and opens the file again,
//! and the audit chain still verifies after every crash.
//!
//! The scenarios, the sweep and the world are the engine test binary's, included here so
//! both backends run exactly the same steps.

#[path = "../../../domovoi-engine/tests/crash/scenarios.rs"]
#[cfg(test)]
mod scenarios;
#[path = "../../../domovoi-engine/tests/crash/sweep.rs"]
#[cfg(test)]
mod sweep;
#[path = "../../../domovoi-engine/tests/crash/world.rs"]
#[cfg(test)]
mod world;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::audit_chain::{ChainStatus, LegacyStatus, VerifyScope};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::T0;
use domovoi_testkit::crash::Reopen;

use self::scenarios::{action_lifecycle, incident_lifecycle};
use self::sweep::{BoxFuture, Scenario, sweep};
use super::sqlite::SqliteStore;

/// One temporary database per sweep case, reopened the way a restarted hub would open it.
struct Sqlite {
    dir: PathBuf,
    next: AtomicUsize,
    current: Mutex<PathBuf>,
}

impl Sqlite {
    fn new(dir: PathBuf) -> Self {
        Self {
            current: Mutex::new(dir.clone()),
            dir,
            next: AtomicUsize::new(0),
        }
    }

    /// A fresh database file, so no case inherits another's state.
    fn make(&self) -> BoxFuture<'static, Arc<SqliteStore>> {
        let path = self.dir.join(format!(
            "case-{}.db",
            self.next.fetch_add(1, Ordering::Relaxed)
        ));
        *self.current.lock().expect("the path lock is not poisoned") = path.clone();
        Box::pin(async move { Arc::new(SqliteStore::new(open(&path).await)) })
    }
}

#[async_trait]
impl Reopen<SqliteStore> for Sqlite {
    async fn reopen(&self, old: Arc<SqliteStore>) -> Arc<SqliteStore> {
        old.close().await;
        let path = self
            .current
            .lock()
            .expect("the path lock is not poisoned")
            .clone();
        let store = Arc::new(SqliteStore::new(open(&path).await));
        // A crash never breaks the chain: what was written before it either chained on or
        // was never written, and the legacy history still matches its anchor.
        let report = store
            .verify_audit(VerifyScope::Full)
            .await
            .expect("the audit history is readable");
        assert!(
            matches!(report.legacy, LegacyStatus::Verified { .. }),
            "the audit history before the chain still verifies: {:?}",
            report.legacy,
        );
        assert!(
            matches!(
                report.chained,
                ChainStatus::Intact { .. } | ChainStatus::Empty
            ),
            "a crash broke the audit chain: {:?}",
            report.chained,
        );
        store
    }
}

/// Opens a database file, or creates it, at the current schema.
async fn open(path: &Path) -> Store {
    Store::open(path, StoreOptions::beside(path, T0))
        .await
        .expect("the database opens")
}

#[tokio::test]
async fn incident_lifecycle_survives_a_crash_at_every_commit_on_sqlite() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let sqlite = Sqlite::new(dir.path().to_path_buf());
    let scenario = Scenario {
        name: "incident_lifecycle",
        run: incident_lifecycle::<SqliteStore>,
        min_commits: 14,
    };
    sweep(&scenario, &|| sqlite.make(), &sqlite).await;
}

#[tokio::test]
async fn action_lifecycle_survives_a_crash_at_every_commit_on_sqlite() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let sqlite = Sqlite::new(dir.path().to_path_buf());
    let scenario = Scenario {
        name: "action_lifecycle",
        run: action_lifecycle::<SqliteStore>,
        min_commits: 24,
    };
    sweep(&scenario, &|| sqlite.make(), &sqlite).await;
}
