//! The staleness sweep: stores the `stale` status that readers already compute.

use domovoi_core::knowledge::{
    Fact, FactUpdate, KnowledgeStatus, KnowledgeWrite, Predicate, Relation, RelationUpdate,
};
use domovoi_core::ports::{
    AggregateRef, FactFilter, JournalDraft, JournalKind, Page, RelationFilter, StateWrite,
    Transaction, Versioned,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use super::decide::apply_update;
use super::error::KnowledgeError;
use super::paths::{NetworkChangeCause, stale_audit};
use super::service::{KnowledgeService, fact_journal, system};

/// Rows read per page while looking for rows past their freshness.
const PAGE: u16 = 500;
/// Most rows one sweep transaction changes.
const BATCH: usize = 100;
/// A read-triggered sweep runs at most this often.
const MIN_INTERVAL: SignedDuration = SignedDuration::from_secs(300);

/// What a sweep changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Facts stored as stale.
    pub marked_stale: u32,
    /// Relations stored as stale.
    pub relations_marked_stale: u32,
}

impl KnowledgeService {
    /// Stores `stale` on every current fact and relation past its freshness. Idempotent; each
    /// batch is its own transaction, so a failure midway leaves a valid state.
    pub async fn sweep(&self) -> Result<SweepReport, KnowledgeError> {
        let _guard = self.lock.lock().await;
        let now = self.now();
        if let Ok(mut last) = self.last_sweep.lock() {
            *last = Some(now);
        }
        let actor = system();
        let mut report = SweepReport::default();

        let due = self.due_facts(now).await?;
        for chunk in due.chunks(BATCH) {
            let mut tx = Transaction::new();
            for found in chunk {
                let update = FactUpdate {
                    status: Some(KnowledgeStatus::Stale),
                    ..FactUpdate::default()
                };
                let after = apply_update(&found.value, &update);
                tx = tx
                    .write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
                        id: found.value.id,
                        expected: found.version,
                        update,
                    }))
                    .journal(fact_journal(&after, "stale", &actor, now));
                if found.value.predicate == Predicate::ManagementPath
                    && let Some(host) = found.value.subject.host()
                {
                    tx = tx.audit(stale_audit(
                        &found.value,
                        host,
                        NetworkChangeCause::Other,
                        now,
                    ));
                }
            }
            self.apply(tx).await?;
            report.marked_stale += u32::try_from(chunk.len()).unwrap_or(u32::MAX);
        }

        let due = self.due_relations(now).await?;
        for chunk in due.chunks(BATCH) {
            let mut tx = Transaction::new();
            for found in chunk {
                tx = tx
                    .write(StateWrite::Knowledge(KnowledgeWrite::RelationUpdate {
                        id: found.value.id,
                        expected: found.version,
                        update: RelationUpdate {
                            status: Some(KnowledgeStatus::Stale),
                            ..RelationUpdate::default()
                        },
                    }))
                    .journal(JournalDraft {
                        aggregate: AggregateRef::relation(found.value.id),
                        kind: JournalKind::RelationChange,
                        payload_v: 1,
                        payload: json!({"change": "stale", "kind": found.value.kind.as_str()}),
                        actor: actor.clone(),
                        occurred_at: now,
                    });
            }
            self.apply(tx).await?;
            report.relations_marked_stale += u32::try_from(chunk.len()).unwrap_or(u32::MAX);
        }
        Ok(report)
    }

    /// Every stored-current fact past its freshness, with its version. Pages through all
    /// current facts by cursor, so no fact is left unexamined however many are stored.
    async fn due_facts(&self, now: Timestamp) -> Result<Vec<Versioned<Fact>>, KnowledgeError> {
        let filter = FactFilter {
            statuses: Some(vec![KnowledgeStatus::Current]),
            ..FactFilter::default()
        };
        let mut page = Page::first(PAGE);
        let mut due = Vec::new();
        loop {
            let read = self.deps.read.facts(&filter, &page).await?;
            for fact in read.items {
                if fact.effective_status(now) == KnowledgeStatus::Stale
                    && let Some(found) = self.deps.read.fact(fact.id).await?
                    && found.value.status == KnowledgeStatus::Current
                {
                    due.push(found);
                }
            }
            match read.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => return Ok(due),
            }
        }
    }

    /// Every stored-current relation past its freshness, with its version.
    async fn due_relations(
        &self,
        now: Timestamp,
    ) -> Result<Vec<Versioned<Relation>>, KnowledgeError> {
        let filter = RelationFilter::default();
        let mut page = Page::first(PAGE);
        let mut due = Vec::new();
        loop {
            let read = self.deps.read.relations(&filter, &page).await?;
            for relation in read.items {
                if relation.status == KnowledgeStatus::Current
                    && relation.effective_status(now) == KnowledgeStatus::Stale
                    && let Some(found) = self.deps.read.relation(relation.id).await?
                {
                    due.push(found);
                }
            }
            match read.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => return Ok(due),
            }
        }
    }

    /// Runs a sweep unless one ran in the last five minutes. A failure is logged and does not
    /// fail the read that asked for it.
    pub(super) async fn sweep_if_due(&self) {
        let now = self.now();
        let due = self
            .last_sweep
            .lock()
            .ok()
            .and_then(|last| *last)
            .is_none_or(|at: Timestamp| at.checked_add(MIN_INTERVAL).is_ok_and(|next| now >= next));
        if due && let Err(error) = self.sweep().await {
            tracing::warn!(%error, "the knowledge sweep failed");
        }
    }
}
