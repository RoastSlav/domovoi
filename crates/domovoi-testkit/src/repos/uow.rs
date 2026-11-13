//! The in-memory unit of work and its readers.

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateRef, CommitError, CommitReceipt, EffectCompletion, EffectDraft, EffectRead,
    EffectRecord, EffectStatus, JournalDraft, JournalEntry, JournalRead, StoreError, Transaction,
    UnitOfWork, Versioned, VersionedRead,
};
use domovoi_core::{EffectId, Incident, IncidentId, Note, NoteId};

use super::incidents::is_active;
use super::uow_apply;
use super::{InMemoryStore, State, version};

fn backend(err: StoreError) -> CommitError {
    CommitError::Backend(err.to_string())
}

fn push_journal(state: &mut State, draft: &JournalDraft) -> u64 {
    let seq = state.journal.len() as u64 + 1;
    state.journal.push(JournalEntry {
        seq,
        aggregate: draft.aggregate.clone(),
        kind: draft.kind.as_str().to_owned(),
        payload_v: draft.payload_v,
        payload: draft.payload.clone(),
        actor: draft.actor.clone(),
        occurred_at: draft.occurred_at,
    });
    seq
}

fn insert_effect(state: &mut State, draft: &EffectDraft) -> Result<(), CommitError> {
    if draft.max_attempts == 0 {
        return Err(CommitError::Constraint(
            "max_attempts must be at least 1".to_owned(),
        ));
    }
    if let Some(key) = &draft.dedup_key
        && state.effects.values().any(|effect| {
            effect.dedup_key.as_ref() == Some(key)
                && matches!(effect.status.as_str(), "pending" | "claimed")
        })
    {
        return Err(CommitError::Constraint("duplicate effect".to_owned()));
    }
    state.effects.insert(
        draft.id,
        EffectRecord {
            id: draft.id,
            kind: draft.kind.as_str().to_owned(),
            aggregate: draft.aggregate.clone(),
            payload: draft.payload.clone(),
            status: EffectStatus::Pending.as_str().to_owned(),
            not_before: draft.not_before,
            attempt: 0,
            max_attempts: draft.max_attempts,
            dedup_key: draft.dedup_key.clone(),
            claim_token: None,
            claim_generation: 0,
            lease_until: None,
            runner_id: None,
            last_error: None,
            created_at: draft.created_at,
            completed_at: None,
        },
    );
    Ok(())
}

fn complete_effect(state: &mut State, completion: &EffectCompletion) -> Result<(), CommitError> {
    let Some(effect) = state.effects.get_mut(&completion.id) else {
        return Err(CommitError::StaleEffect {
            effect: completion.id,
        });
    };
    let held = match completion.claim_token {
        Some(token) => effect.status == "claimed" && effect.claim_token == Some(token),
        None => effect.status == "pending",
    };
    if !held {
        return Err(CommitError::StaleEffect {
            effect: completion.id,
        });
    }
    effect.status = EffectStatus::Done.as_str().to_owned();
    effect.completed_at = Some(completion.at);
    Ok(())
}

impl InMemoryStore {
    /// Test seam: claims a pending effect as the effect runner will, bumping its generation.
    pub fn claim_effect(
        &self,
        id: EffectId,
        token: domovoi_core::ports::ClaimToken,
        lease_until: jiff::Timestamp,
    ) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let effect = state.effects.get_mut(&id).ok_or(StoreError::NotFound)?;
        effect.status = EffectStatus::Claimed.as_str().to_owned();
        effect.claim_token = Some(token);
        effect.claim_generation = effect.claim_generation.saturating_add(1);
        effect.lease_until = Some(lease_until);
        effect.attempt = effect.attempt.saturating_add(1);
        Ok(())
    }
}

#[async_trait]
impl UnitOfWork for InMemoryStore {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        if tx.is_empty() {
            return Ok(CommitReceipt::default());
        }
        for draft in &tx.new_effects {
            if draft.max_attempts == 0 {
                return Err(CommitError::Constraint(
                    "max_attempts must be at least 1".to_owned(),
                ));
            }
        }
        let mut state = self.begin().map_err(backend)?;
        let mut draft = (*state).clone();
        let mut receipt = CommitReceipt::default();
        let mut mirrors = Vec::new();
        for write in &tx.writes {
            uow_apply::apply(&mut draft, write, &mut receipt, &mut mirrors)?;
        }
        for journal in &mirrors {
            receipt.journal_seqs.push(push_journal(&mut draft, journal));
        }
        for journal in &tx.journal {
            receipt.journal_seqs.push(push_journal(&mut draft, journal));
        }
        for audit in &tx.audit {
            draft.audit.push(audit.event.clone());
            receipt.audit_seqs.push(draft.audit.len() as u64);
        }
        for effect in &tx.new_effects {
            insert_effect(&mut draft, effect)?;
        }
        for completion in &tx.completed_effects {
            complete_effect(&mut draft, completion)?;
        }
        *state = draft;
        Ok(receipt)
    }
}

#[async_trait]
impl VersionedRead for InMemoryStore {
    async fn incident(&self, id: IncidentId) -> Result<Option<Versioned<Incident>>, StoreError> {
        let state = self.begin()?;
        let Some(incident) = state.incidents.get(&id) else {
            return Ok(None);
        };
        Ok(Some(Versioned {
            value: incident.clone(),
            version: version(&state, &AggregateRef::incident(id)),
        }))
    }

    async fn active_incidents(&self) -> Result<Vec<Versioned<Incident>>, StoreError> {
        let state = self.begin()?;
        let mut active: Vec<Incident> = state
            .incidents
            .values()
            .filter(|incident| is_active(incident))
            .cloned()
            .collect();
        active.sort_by(|a, b| {
            b.opened_at()
                .cmp(&a.opened_at())
                .then_with(|| b.id().cmp(&a.id()))
        });
        Ok(active
            .into_iter()
            .map(|incident| Versioned {
                version: version(&state, &AggregateRef::incident(incident.id())),
                value: incident,
            })
            .collect())
    }

    async fn note(&self, id: NoteId) -> Result<Option<Versioned<Note>>, StoreError> {
        let state = self.begin()?;
        let Some(note) = state.notes.get(&id) else {
            return Ok(None);
        };
        Ok(Some(Versioned {
            value: note.clone(),
            version: version(&state, &AggregateRef::note(id)),
        }))
    }
}

#[async_trait]
impl JournalRead for InMemoryStore {
    async fn entries_after(
        &self,
        after_seq: u64,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .journal
            .iter()
            .filter(|entry| entry.seq > after_seq)
            .take(limit_of(limit))
            .cloned()
            .collect())
    }

    async fn entries_for(
        &self,
        aggregate: &AggregateRef,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .journal
            .iter()
            .filter(|entry| entry.aggregate == *aggregate)
            .take(limit_of(limit))
            .cloned()
            .collect())
    }

    async fn head_seq(&self) -> Result<u64, StoreError> {
        Ok(self.begin()?.journal.last().map_or(0, |entry| entry.seq))
    }
}

fn limit_of(limit: u16) -> usize {
    limit.clamp(1, domovoi_core::ports::Page::MAX_LIMIT).into()
}

#[async_trait]
impl EffectRead for InMemoryStore {
    async fn effect(&self, id: EffectId) -> Result<Option<EffectRecord>, StoreError> {
        Ok(self.begin()?.effects.get(&id).cloned())
    }

    async fn effects_with_status(
        &self,
        status: EffectStatus,
        limit: u16,
    ) -> Result<Vec<EffectRecord>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .effects
            .values()
            .filter(|effect| effect.status == status.as_str())
            .take(limit_of(limit))
            .cloned()
            .collect())
    }
}
