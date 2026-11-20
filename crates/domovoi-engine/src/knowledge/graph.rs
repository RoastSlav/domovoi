//! Relations and entities.

use domovoi_core::knowledge::{
    Entity, EvidenceRef, FactSource, KnowledgeStatus, KnowledgeWrite, Relation, RelationKind,
    RelationUpdate, SubjectRef, clean_entity_name, source_allows_actor, valid_entity_kind,
};
use domovoi_core::ports::{
    AggregateRef, AuditDraft, JournalDraft, JournalKind, StateWrite, Transaction, Versioned,
};
use domovoi_core::{Actor, EntityId, RelationId, UserId};
use jiff::Timestamp;
use serde_json::json;

use super::error::KnowledgeError;
use super::service::KnowledgeService;

/// A relation someone wants recorded.
#[derive(Debug, Clone)]
pub struct RelationDraft {
    /// The source end.
    pub from: SubjectRef,
    /// What the edge means.
    pub kind: RelationKind,
    /// The target end.
    pub to: SubjectRef,
    /// Where it came from; owner actions force `User`.
    pub source: FactSource,
    /// When it was observed; defaults to now.
    pub observed_at: Option<Timestamp>,
    /// What it was derived from.
    pub evidence: Option<EvidenceRef>,
}

fn relation_journal(
    id: RelationId,
    change: &str,
    kind: RelationKind,
    actor: &Actor,
    at: Timestamp,
) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::relation(id),
        kind: JournalKind::RelationChange,
        payload_v: 1,
        payload: json!({"change": change, "kind": kind.as_str()}),
        actor: actor.clone(),
        occurred_at: at,
    }
}

fn entity_journal(
    id: EntityId,
    change: &str,
    kind: &str,
    actor: &Actor,
    at: Timestamp,
) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::entity(id),
        kind: JournalKind::EntityChange,
        payload_v: 1,
        payload: json!({"change": change, "kind": kind}),
        actor: actor.clone(),
        occurred_at: at,
    }
}

fn audit(
    kind: &str,
    subject: String,
    detail_kind: &str,
    actor: &Actor,
    at: Timestamp,
) -> AuditDraft {
    AuditDraft::new(
        actor.clone(),
        at,
        kind.to_owned(),
        subject,
        json!({"kind": detail_kind}),
    )
}

impl KnowledgeService {
    async fn load_relation(&self, id: RelationId) -> Result<Versioned<Relation>, KnowledgeError> {
        self.deps
            .read
            .relation(id)
            .await?
            .ok_or(KnowledgeError::NotFound)
    }

    async fn load_entity(&self, id: EntityId) -> Result<Versioned<Entity>, KnowledgeError> {
        self.deps
            .read
            .entity(id)
            .await?
            .ok_or(KnowledgeError::NotFound)
    }

    /// Writes or refreshes one edge. The caller holds the mutation lock.
    async fn write_relation(
        &self,
        draft: &RelationDraft,
        actor: &Actor,
        audit_kind: Option<&str>,
    ) -> Result<Versioned<Relation>, KnowledgeError> {
        if draft.from == draft.to {
            return Err(KnowledgeError::InvalidInput(
                "a relation needs two different subjects".to_owned(),
            ));
        }
        let now = self.now();
        let observed = draft.observed_at.unwrap_or(now);
        let existing = self
            .deps
            .read
            .live_relation(&draft.from, draft.kind, &draft.to)
            .await?;
        let (id, tx) = match existing {
            Some(found) => {
                let id = found.value.id;
                let status = (found.value.status == KnowledgeStatus::Stale)
                    .then_some(KnowledgeStatus::Current);
                let tx = Transaction::new()
                    .write(StateWrite::Knowledge(KnowledgeWrite::RelationUpdate {
                        id,
                        expected: found.version,
                        update: RelationUpdate {
                            status,
                            observed_at: Some(found.value.observed_at.max(observed)),
                            retraction: None,
                        },
                    }))
                    .journal(relation_journal(id, "refreshed", draft.kind, actor, now));
                (id, tx)
            }
            None => {
                let id = RelationId::new();
                let relation = Relation {
                    id,
                    from: Some(draft.from.clone()),
                    kind: draft.kind,
                    to: Some(draft.to.clone()),
                    source: draft.source,
                    status: KnowledgeStatus::Current,
                    observed_at: observed,
                    evidence: draft.evidence.clone(),
                    actor: actor.clone(),
                    created_at: now,
                    retracted_at: None,
                    retracted_by: None,
                    content_purged_at: None,
                };
                let tx = Transaction::new()
                    .write(StateWrite::Knowledge(KnowledgeWrite::RelationInsert(
                        relation,
                    )))
                    .journal(relation_journal(id, "recorded", draft.kind, actor, now));
                (id, tx)
            }
        };
        let tx = match audit_kind {
            Some(kind) => tx.audit(audit(
                kind,
                format!("relation:{id}"),
                draft.kind.as_str(),
                actor,
                now,
            )),
            None => tx,
        };
        self.apply(tx).await?;
        self.load_relation(id).await
    }

    /// Records an edge written by a system component. `source = user` is refused here.
    pub async fn record_relation(
        &self,
        draft: RelationDraft,
        actor: &Actor,
    ) -> Result<Versioned<Relation>, KnowledgeError> {
        if draft.source == FactSource::User || !source_allows_actor(draft.source, actor) {
            return Err(KnowledgeError::ActorNotAllowed {
                source: draft.source,
            });
        }
        let _guard = self.lock.lock().await;
        self.write_relation(&draft, actor, None).await
    }

    /// Records an edge the owner states. The source is `user` whatever the draft says.
    pub async fn assert_relation(
        &self,
        mut draft: RelationDraft,
        user: UserId,
    ) -> Result<Versioned<Relation>, KnowledgeError> {
        draft.source = FactSource::User;
        let _guard = self.lock.lock().await;
        self.write_relation(
            &draft,
            &Actor::User(user),
            Some("knowledge.relation.asserted"),
        )
        .await
    }

    /// Withdraws a live edge.
    pub async fn retract_relation(
        &self,
        id: RelationId,
        expected: u64,
        user: UserId,
    ) -> Result<Versioned<Relation>, KnowledgeError> {
        let _guard = self.lock.lock().await;
        let now = self.now();
        let actor = Actor::User(user);
        let found = self.load_relation(id).await?;
        if found.version != expected {
            return Err(KnowledgeError::Conflict);
        }
        if !found.value.status.is_live() {
            return Err(KnowledgeError::NotLive {
                status: found.value.status,
            });
        }
        let kind = found.value.kind;
        let tx = Transaction::new()
            .write(StateWrite::Knowledge(KnowledgeWrite::RelationUpdate {
                id,
                expected,
                update: RelationUpdate {
                    status: Some(KnowledgeStatus::Retracted),
                    observed_at: None,
                    retraction: Some((now, actor.clone())),
                },
            }))
            .journal(relation_journal(id, "retracted", kind, &actor, now))
            .audit(audit(
                "knowledge.relation.retracted",
                format!("relation:{id}"),
                kind.as_str(),
                &actor,
                now,
            ));
        self.apply(tx).await?;
        self.load_relation(id).await
    }

    async fn name_taken(
        &self,
        name: &str,
        except: Option<EntityId>,
    ) -> Result<bool, KnowledgeError> {
        let lower = name.to_lowercase();
        Ok(self
            .deps
            .read
            .entities()
            .await?
            .iter()
            .any(|e| Some(e.value.id) != except && e.value.name.to_lowercase() == lower))
    }

    /// Creates an entity: a subject only the owner can invent.
    pub async fn create_entity(
        &self,
        kind: &str,
        name: &str,
        user: UserId,
    ) -> Result<Versioned<Entity>, KnowledgeError> {
        if !valid_entity_kind(kind) {
            return Err(KnowledgeError::InvalidInput(
                "an entity kind is 1 to 32 characters of a-z, 0-9, _ or -".to_owned(),
            ));
        }
        let name = clean_entity_name(name).ok_or_else(|| {
            KnowledgeError::InvalidInput("an entity name is 1 to 80 characters".to_owned())
        })?;
        let _guard = self.lock.lock().await;
        if self.name_taken(&name, None).await? {
            return Err(KnowledgeError::DuplicateEntity);
        }
        let now = self.now();
        let actor = Actor::User(user);
        let id = EntityId::new();
        let tx = Transaction::new()
            .write(StateWrite::Knowledge(KnowledgeWrite::EntityInsert(
                Entity {
                    id,
                    kind: kind.to_owned(),
                    name,
                    created_by: actor.clone(),
                    created_at: now,
                },
            )))
            .journal(entity_journal(id, "created", kind, &actor, now))
            .audit(audit(
                "knowledge.entity.created",
                format!("entity:{id}"),
                kind,
                &actor,
                now,
            ));
        self.apply(tx).await?;
        self.load_entity(id).await
    }

    /// Renames an entity.
    pub async fn rename_entity(
        &self,
        id: EntityId,
        expected: u64,
        name: &str,
        user: UserId,
    ) -> Result<Versioned<Entity>, KnowledgeError> {
        let name = clean_entity_name(name).ok_or_else(|| {
            KnowledgeError::InvalidInput("an entity name is 1 to 80 characters".to_owned())
        })?;
        let _guard = self.lock.lock().await;
        let found = self.load_entity(id).await?;
        if found.version != expected {
            return Err(KnowledgeError::Conflict);
        }
        if self.name_taken(&name, Some(id)).await? {
            return Err(KnowledgeError::DuplicateEntity);
        }
        let now = self.now();
        let actor = Actor::User(user);
        let kind = found.value.kind;
        let tx = Transaction::new()
            .write(StateWrite::Knowledge(KnowledgeWrite::EntityRename {
                id,
                expected,
                name,
            }))
            .journal(entity_journal(id, "renamed", &kind, &actor, now))
            .audit(audit(
                "knowledge.entity.renamed",
                format!("entity:{id}"),
                &kind,
                &actor,
                now,
            ));
        self.apply(tx).await?;
        self.load_entity(id).await
    }

    /// Deletes an entity nothing live refers to.
    pub async fn delete_entity(
        &self,
        id: EntityId,
        expected: u64,
        user: UserId,
    ) -> Result<(), KnowledgeError> {
        let _guard = self.lock.lock().await;
        let found = self.load_entity(id).await?;
        if found.version != expected {
            return Err(KnowledgeError::Conflict);
        }
        if self.deps.read.entity_in_use(id).await? {
            return Err(KnowledgeError::EntityInUse);
        }
        let now = self.now();
        let actor = Actor::User(user);
        let kind = found.value.kind;
        let tx = Transaction::new()
            .write(StateWrite::Knowledge(KnowledgeWrite::EntityDelete {
                id,
                expected,
            }))
            .journal(entity_journal(id, "deleted", &kind, &actor, now))
            .audit(audit(
                "knowledge.entity.deleted",
                format!("entity:{id}"),
                &kind,
                &actor,
                now,
            ));
        self.apply(tx).await?;
        Ok(())
    }
}
