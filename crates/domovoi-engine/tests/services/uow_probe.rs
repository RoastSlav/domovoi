//! Test doubles for the action writer's unit of work.
//!
//! Real atomicity of the store is proved by the unit-of-work contract tests and the
//! SQLite tests; these doubles only let a test watch what the writer commits, refuse a
//! commit, or win a race against it.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ActionRequest;
use domovoi_core::ports::{ActionRepo, CommitError, CommitReceipt, Transaction, UnitOfWork};
use domovoi_testkit::InMemoryStore;

/// Forwards every transaction to an in-memory store and keeps a copy of each commit.
pub struct RecordingUow {
    store: Arc<InMemoryStore>,
    commits: Arc<Mutex<Vec<Transaction>>>,
}

impl RecordingUow {
    /// A recording unit of work over `store`.
    pub fn new(store: Arc<InMemoryStore>) -> Self {
        Self {
            store,
            commits: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Every transaction committed so far, oldest first.
    pub fn commits(&self) -> Vec<Transaction> {
        self.commits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl UnitOfWork for RecordingUow {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        self.commits
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(tx.clone());
        UnitOfWork::commit(&*self.store, tx).await
    }
}

/// Refuses any transaction that carries an audit entry, and applies nothing. A store with
/// no such entry is forwarded, so the double is only strict about audit writes.
pub struct RefusingAuditUow {
    store: Arc<InMemoryStore>,
}

impl RefusingAuditUow {
    /// A unit of work over `store` that refuses every audit write.
    pub fn new(store: Arc<InMemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl UnitOfWork for RefusingAuditUow {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        if !tx.audit.is_empty() {
            return Err(CommitError::Constraint("audit refused".to_owned()));
        }
        UnitOfWork::commit(&*self.store, tx).await
    }
}

/// Lets a rival decision win between the writer's read and its commit: the first
/// transaction that carries an audit entry is preceded by writing `rival` over the stored
/// action, so the writer's status guard no longer matches. Every transaction is then
/// forwarded, which is how a lost race reaches the writer's error mapping.
pub struct StaleRaceUow {
    store: Arc<InMemoryStore>,
    rival: ActionRequest,
    raced: Mutex<bool>,
}

impl StaleRaceUow {
    /// A unit of work over `store` whose first commit loses the race to `rival`, a copy
    /// of the stored action in another status.
    pub fn new(store: Arc<InMemoryStore>, rival: ActionRequest) -> Self {
        Self {
            store,
            rival,
            raced: Mutex::new(false),
        }
    }
}

#[async_trait]
impl UnitOfWork for StaleRaceUow {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        let race = !tx.audit.is_empty() && {
            let mut raced = self
                .raced
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let first = !*raced;
            *raced = true;
            first
        };
        if race {
            ActionRepo::update_from(&*self.store, &self.rival, "proposed")
                .await
                .map_err(|err| CommitError::Backend(err.to_string()))?;
        }
        UnitOfWork::commit(&*self.store, tx).await
    }
}
