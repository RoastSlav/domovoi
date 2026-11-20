//! Owner actions on one fact: retract and verify.

use domovoi_core::knowledge::{Fact, FactUpdate, KnowledgeStatus, KnowledgeWrite, Predicate};
use domovoi_core::ports::{StateWrite, Transaction, Versioned};
use domovoi_core::{Actor, FactRecordId, UserId};

use super::decide::{apply_update, plan_settle};
use super::error::KnowledgeError;
use super::service::{KnowledgeService, fact_audit, fact_journal, with_update};

impl KnowledgeService {
    /// Withdraws a live fact of any predicate. It leaves search and listings at once; its
    /// value is cleared after the retention period. Retraction only removes trust, so it
    /// needs no coverage and no confirmation.
    pub async fn retract(
        &self,
        id: FactRecordId,
        expected: u64,
        user: UserId,
    ) -> Result<Versioned<Fact>, KnowledgeError> {
        let _guard = self.lock.lock().await;
        let now = self.now();
        let actor = Actor::User(user);
        let found = self.load_live(id, expected).await?;
        let update = FactUpdate {
            status: Some(KnowledgeStatus::Retracted),
            retraction: Some((now, actor.clone())),
            ..FactUpdate::default()
        };
        let after = apply_update(&found.value, &update);
        let mut tx = Transaction::new()
            .write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
                id,
                expected,
                update,
            }))
            .journal(fact_journal(&after, "retracted", &actor, now))
            .audit(fact_audit("knowledge.fact.retracted", &after, &actor, now));
        // A dispute that only this fact kept alive is over.
        let remaining: Vec<Versioned<Fact>> = self
            .deps
            .read
            .live_facts(&found.value.subject, found.value.predicate)
            .await?
            .into_iter()
            .filter(|v| v.value.id != id)
            .collect();
        for plan in plan_settle(&remaining) {
            tx = with_update(tx, &plan, &actor, now);
        }
        self.apply(tx).await?;
        self.load(id).await
    }

    /// Marks a live fact as checked now and revives it when stale. A management path is
    /// confirmed, never verified.
    pub async fn verify(
        &self,
        id: FactRecordId,
        expected: u64,
        user: UserId,
    ) -> Result<Versioned<Fact>, KnowledgeError> {
        let _guard = self.lock.lock().await;
        let now = self.now();
        let actor = Actor::User(user);
        let found = self.load(id).await?;
        if found.version != expected {
            return Err(KnowledgeError::Conflict);
        }
        if found.value.predicate == Predicate::ManagementPath {
            return Err(KnowledgeError::UseConfirmation);
        }
        if !found.value.status.is_live() {
            return Err(KnowledgeError::NotLive {
                status: found.value.status,
            });
        }
        let update = FactUpdate {
            status: (found.value.status == KnowledgeStatus::Stale)
                .then_some(KnowledgeStatus::Current),
            last_verified_at: Some(now),
            ..FactUpdate::default()
        };
        let after = apply_update(&found.value, &update);
        let tx = Transaction::new()
            .write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
                id,
                expected,
                update,
            }))
            .journal(fact_journal(&after, "verified", &actor, now))
            .audit(fact_audit("knowledge.fact.verified", &after, &actor, now));
        self.apply(tx).await?;
        self.load(id).await
    }
}
