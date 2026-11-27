//! The knowledge service: the one writer of facts, relations and entities.
//!
//! A request/response service, not a task. Every mutating call holds one mutex across read,
//! decision and commit, and commits through the unit of work, so a change, its journal entry
//! and (for owner actions) its audit entry land together or not at all. Journal and audit
//! payloads name ids, predicates and subjects, never a value, because they outlive the
//! retention purge.

use std::sync::Arc;

use domovoi_core::knowledge::{
    EvidenceRef, Fact, FactSource, KnowledgeWrite, Predicate, SubjectRef, source_allows_actor,
    window_end,
};
use domovoi_core::ports::{
    AggregateRef, AuditDraft, Clock, CommitReceipt, FactFilter, FactHit, JournalDraft, JournalKind,
    KnowledgeRead, ManagementCoverage, Page, Paged, StateWrite, Transaction, UnitOfWork, Versioned,
};
use domovoi_core::{Actor, FactRecordId, HostId, UserId};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::decide::{PlannedUpdate, RecordEffect, ValidDraft, plan_record};
use super::error::KnowledgeError;

/// What the service needs.
#[derive(Clone)]
pub struct KnowledgeDeps {
    /// Commits every change.
    pub uow: Arc<dyn UnitOfWork>,
    /// Reads facts, relations and entities.
    pub read: Arc<dyn KnowledgeRead>,
    /// Checks management-path coverage.
    pub coverage: Arc<dyn ManagementCoverage>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A fact someone wants recorded.
#[derive(Debug, Clone)]
pub struct FactDraft {
    /// What it is about.
    pub subject: SubjectRef,
    /// What it says.
    pub predicate: Predicate,
    /// The value, validated for the predicate.
    pub value: Value,
    /// Where it came from; owner actions force `User`.
    pub source: FactSource,
    /// When it was observed; defaults to now.
    pub observed_at: Option<Timestamp>,
    /// After this instant it no longer holds.
    pub valid_until: Option<Timestamp>,
    /// What it was derived from.
    pub evidence: Option<EvidenceRef>,
}

/// What a record produced.
#[derive(Debug, Clone)]
pub struct Recorded {
    /// The fact the call concerns, as stored.
    pub fact: Versioned<Fact>,
    /// What the call did.
    pub effect: RecordEffect,
}

/// The knowledge service. See the module documentation.
pub struct KnowledgeService {
    pub(super) deps: KnowledgeDeps,
    pub(super) lock: tokio::sync::Mutex<()>,
    pub(super) last_sweep: std::sync::Mutex<Option<Timestamp>>,
}

/// The system actor for sweeps and service-internal changes.
pub(super) fn system() -> Actor {
    Actor::System(domovoi_core::SYSTEM_KNOWLEDGE.to_owned())
}

/// The journal payload of one fact change. It carries no value.
pub(super) fn fact_payload(fact: &Fact, change: &str) -> Value {
    json!({
        "change": change,
        "predicate": fact.predicate.as_str(),
        "subject": fact.subject.to_string(),
        "status": fact.status.as_str(),
        "source": fact.source.as_str(),
        "supersedes": fact.supersedes.map(|id| id.to_string()),
        "evidence": fact.evidence.as_ref().map(ToString::to_string),
    })
}

/// A journal entry for one fact change.
pub(super) fn fact_journal(
    fact: &Fact,
    change: &str,
    actor: &Actor,
    at: Timestamp,
) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::fact(fact.id),
        kind: JournalKind::FactChange,
        payload_v: 1,
        payload: fact_payload(fact, change),
        actor: actor.clone(),
        occurred_at: at,
    }
}

/// An audit entry that names the predicate and subject but no value.
pub(super) fn fact_audit(kind: &str, fact: &Fact, actor: &Actor, at: Timestamp) -> AuditDraft {
    AuditDraft::new(
        actor.clone(),
        at,
        kind.to_owned(),
        format!("fact:{}", fact.id),
        json!({"predicate": fact.predicate.as_str(), "subject": fact.subject.to_string()}),
    )
}

/// Adds the write and journal entry of a planned update.
pub(super) fn with_update(
    tx: Transaction,
    plan: &PlannedUpdate,
    actor: &Actor,
    at: Timestamp,
) -> Transaction {
    tx.write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
        id: plan.id,
        expected: plan.expected,
        update: plan.update.clone(),
    }))
    .journal(fact_journal(&plan.after, plan.change, actor, at))
}

impl KnowledgeService {
    /// A service over `deps`.
    #[must_use]
    pub fn new(deps: KnowledgeDeps) -> Self {
        Self {
            deps,
            lock: tokio::sync::Mutex::new(()),
            last_sweep: std::sync::Mutex::new(None),
        }
    }

    /// Whether a coverage check can currently succeed at all.
    #[must_use]
    pub fn coverage_available(&self) -> bool {
        self.deps.coverage.is_available()
    }

    pub(super) fn now(&self) -> Timestamp {
        self.deps.clock.now()
    }

    pub(super) async fn apply(&self, tx: Transaction) -> Result<CommitReceipt, KnowledgeError> {
        Ok(self.deps.uow.commit(tx).await?)
    }

    pub(super) async fn load(&self, id: FactRecordId) -> Result<Versioned<Fact>, KnowledgeError> {
        self.deps
            .read
            .fact(id)
            .await?
            .ok_or(KnowledgeError::NotFound)
    }

    /// Loads a fact the caller expects to be at `expected` and live.
    pub(super) async fn load_live(
        &self,
        id: FactRecordId,
        expected: u64,
    ) -> Result<Versioned<Fact>, KnowledgeError> {
        let found = self.load(id).await?;
        if found.version != expected {
            return Err(KnowledgeError::Conflict);
        }
        if !found.value.status.is_live() {
            return Err(KnowledgeError::NotLive {
                status: found.value.status,
            });
        }
        Ok(found)
    }

    /// Checks a draft against its predicate and defaults its time.
    pub(super) fn validate(
        &self,
        draft: &FactDraft,
        source: FactSource,
    ) -> Result<ValidDraft, KnowledgeError> {
        if !draft.predicate.allows_subject(&draft.subject) {
            return Err(KnowledgeError::SubjectNotAllowed {
                predicate: draft.predicate,
            });
        }
        if draft.predicate == Predicate::HostPlatform
            && !matches!(source, FactSource::Discovered | FactSource::Integration)
        {
            return Err(KnowledgeError::InvalidInput(
                "host_platform is observed, not asserted".to_owned(),
            ));
        }
        let value = draft.predicate.validate(&draft.subject, &draft.value)?;
        let valid_until = if draft.predicate == Predicate::MaintenanceWindow {
            window_end(&value)
        } else {
            draft.valid_until
        };
        Ok(ValidDraft {
            subject: draft.subject.clone(),
            predicate: draft.predicate,
            value,
            source,
            observed_at: draft.observed_at.unwrap_or_else(|| self.now()),
            valid_until,
            evidence: draft.evidence.clone(),
        })
    }

    /// Plans, commits and reads back one record. The caller holds the mutation lock.
    /// `audit` names the audit kind and an optional `from` fact for owner actions.
    pub(super) async fn write_record(
        &self,
        draft: &ValidDraft,
        actor: &Actor,
        audit: Option<(&str, Option<FactRecordId>)>,
    ) -> Result<Recorded, KnowledgeError> {
        let now = self.now();
        let live = self
            .deps
            .read
            .live_facts(&draft.subject, draft.predicate)
            .await?;
        let plan = plan_record(&live, draft, actor, now, FactRecordId::new());
        if plan.effect != RecordEffect::Unchanged {
            let mut tx = Transaction::new();
            for update in &plan.updates {
                tx = with_update(tx, update, actor, now);
            }
            for fact in &plan.inserts {
                tx = tx
                    .write(StateWrite::Knowledge(KnowledgeWrite::FactInsert(
                        fact.clone(),
                    )))
                    .journal(fact_journal(fact, "recorded", actor, now));
            }
            if let Some((kind, from)) = audit {
                let target = plan.inserts.first().or_else(|| {
                    plan.updates
                        .iter()
                        .find(|u| u.id == plan.result)
                        .map(|u| &u.after)
                });
                if let Some(fact) = target {
                    let mut entry = fact_audit(kind, fact, actor, now);
                    if let Some(from) = from {
                        entry.detail["from"] = json!(from.to_string());
                    }
                    tx = tx.audit(entry);
                }
            }
            self.apply(tx).await?;
        }
        let fact = self.load(plan.result).await?;
        Ok(Recorded {
            fact,
            effect: plan.effect,
        })
    }

    /// Records a fact written by a system component. `source = user` is refused for every
    /// actor here: only [`assert_fact`](Self::assert_fact) writes owner facts.
    pub async fn record(
        &self,
        draft: FactDraft,
        actor: &Actor,
    ) -> Result<Recorded, KnowledgeError> {
        if draft.source == FactSource::User || !source_allows_actor(draft.source, actor) {
            return Err(KnowledgeError::ActorNotAllowed {
                source: draft.source,
            });
        }
        let valid = self.validate(&draft, draft.source)?;
        let _guard = self.lock.lock().await;
        self.write_record(&valid, actor, None).await
    }

    /// Whether the owner could state `draft`, without writing anything: what a caller checks
    /// before it asks for a password.
    pub fn check_owner_draft(&self, draft: &FactDraft) -> Result<(), KnowledgeError> {
        self.validate(draft, FactSource::User).map(drop)
    }

    /// Records a fact the owner states. The source is `user` whatever the draft says.
    pub async fn assert_fact(
        &self,
        draft: FactDraft,
        user: UserId,
    ) -> Result<Recorded, KnowledgeError> {
        let valid = self.validate(&draft, FactSource::User)?;
        let _guard = self.lock.lock().await;
        self.write_record(
            &valid,
            &Actor::User(user),
            Some(("knowledge.fact.asserted", None)),
        )
        .await
    }

    /// Re-records an existing fact as the owner's own. The only way a fact from another
    /// source becomes `user`-sourced.
    pub async fn accept(
        &self,
        id: FactRecordId,
        expected: u64,
        user: UserId,
    ) -> Result<Recorded, KnowledgeError> {
        let _guard = self.lock.lock().await;
        let found = self.load_live(id, expected).await?;
        let fact = &found.value;
        let value = fact.value.clone().ok_or(KnowledgeError::NotLive {
            status: fact.status,
        })?;
        let draft = FactDraft {
            subject: fact.subject.clone(),
            predicate: fact.predicate,
            value,
            source: FactSource::User,
            observed_at: None,
            valid_until: fact.valid_until,
            evidence: fact.evidence.clone(),
        };
        let valid = self.validate(&draft, FactSource::User)?;
        self.write_record(
            &valid,
            &Actor::User(user),
            Some(("knowledge.fact.accepted", Some(id))),
        )
        .await
    }

    /// A page of facts, newest first. Stored statuses; use `effective_status` to show them.
    pub async fn facts(
        &self,
        filter: &FactFilter,
        page: &Page,
    ) -> Result<Paged<Fact>, KnowledgeError> {
        self.sweep_if_due().await;
        Ok(self.deps.read.facts(filter, page).await?)
    }

    /// Full-text search over live facts.
    pub async fn search(
        &self,
        query: &str,
        host: Option<HostId>,
        limit: u16,
    ) -> Result<Vec<FactHit>, KnowledgeError> {
        self.sweep_if_due().await;
        Ok(self.deps.read.search_facts(query, host, limit).await?)
    }
}
