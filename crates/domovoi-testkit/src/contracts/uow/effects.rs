//! Unit-of-work contracts about effects.

use domovoi_core::ports::{
    AggregateRef, ClaimToken, CommitError, EffectCompletion, EffectDraft, EffectKind, EffectRecord,
    EffectStatus, Expected, StateWrite, Transaction,
};
use domovoi_core::{EffectId, IncidentId, Note, NoteId};
use serde_json::json;

use super::super::ContractResult;
use super::{UowSubject, lease};
use crate::build::{T0, at, host, open_incident};

fn wake(aggregate: &AggregateRef, at_secs: i64) -> EffectDraft {
    EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Wake,
        aggregate: aggregate.clone(),
        payload: json!({}),
        not_before: at(at_secs),
        max_attempts: 3,
        dedup_key: None,
        created_at: at(at_secs),
    }
}

/// A wake effect with an id and creation time chosen by the test.
fn fixed_wake(id: u128, aggregate: &AggregateRef, at_secs: i64) -> EffectDraft {
    let mut draft = wake(aggregate, at_secs);
    draft.id = EffectId::from_uuid(uuid::Uuid::from_u128(id));
    draft
}

fn ids(records: &[EffectRecord]) -> Vec<EffectId> {
    records.iter().map(|record| record.id).collect()
}

fn note(host_id: domovoi_core::HostId) -> Note {
    Note {
        id: NoteId::new(),
        host: Some(host_id),
        title: "Runbook".to_owned(),
        body_markdown: "Steps".to_owned(),
        tags: Vec::new(),
        updated_at: T0,
    }
}

/// A dedup key is refused while an effect with it is pending, and accepted after completion.
pub async fn duplicate_dedup_key_is_refused_while_pending(
    subject: &dyn UowSubject,
) -> ContractResult {
    let aggregate = AggregateRef::incident(IncidentId::new());
    let mut first = wake(&aggregate, 1);
    first.dedup_key = Some("one".to_owned());
    let mut second = wake(&aggregate, 2);
    second.dedup_key = Some("one".to_owned());

    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .effect(first.clone())
                .effect(second.clone()),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Constraint(message)) if message == "duplicate effect"
    ));
    assert_eq!(subject.effects().effect(first.id).await?, None);

    subject
        .uow()
        .commit(Transaction::new().effect(first.clone()))
        .await?;
    subject
        .uow()
        .commit(Transaction::new().complete(EffectCompletion {
            id: first.id,
            claim_token: None,
            at: at(5),
        }))
        .await?;
    let accepted = subject
        .uow()
        .commit(Transaction::new().effect(second.clone()))
        .await?;
    assert_eq!(accepted.journal_seqs.len(), 0);
    let record = subject
        .effects()
        .effect(second.id)
        .await?
        .ok_or_else(|| super::super::ContractError::Setup("effect missing".to_owned()))?;
    assert_eq!(record.status, "pending");
    Ok(())
}

/// A completion needs the current claim token, and a stale one rolls back the whole transaction.
pub async fn effect_completion_needs_the_current_claim_token(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let aggregate = AggregateRef::incident(IncidentId::new());
    let effect = wake(&aggregate, 1);
    subject
        .uow()
        .commit(Transaction::new().effect(effect.clone()))
        .await?;
    let token_a = ClaimToken::new();
    subject.force_claim(effect.id, token_a, lease(60)).await?;

    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .complete(EffectCompletion {
                    id: effect.id,
                    claim_token: Some(ClaimToken::new()),
                    at: at(7),
                })
                .write(StateWrite::Incident {
                    incident: incident.clone(),
                    expected: Expected::New,
                }),
        )
        .await;
    assert!(matches!(failed, Err(CommitError::StaleEffect { .. })));
    assert_eq!(subject.versions().incident(incident.id()).await?, None);

    subject
        .uow()
        .commit(Transaction::new().complete(EffectCompletion {
            id: effect.id,
            claim_token: Some(token_a),
            at: at(8),
        }))
        .await?;
    let record = subject
        .effects()
        .effect(effect.id)
        .await?
        .ok_or_else(|| super::super::ContractError::Setup("effect missing".to_owned()))?;
    assert_eq!(record.status, "done");
    assert_eq!(record.completed_at, Some(at(8)));

    let second = wake(&aggregate, 2);
    subject
        .uow()
        .commit(Transaction::new().effect(second.clone()))
        .await?;
    subject
        .force_claim(second.id, ClaimToken::new(), lease(60))
        .await?;
    let failed = subject
        .uow()
        .commit(Transaction::new().complete(EffectCompletion {
            id: second.id,
            claim_token: None,
            at: at(9),
        }))
        .await;
    assert!(matches!(failed, Err(CommitError::StaleEffect { .. })));
    Ok(())
}

/// Completing an unclaimed effect without a token marks it done.
pub async fn completing_an_unclaimed_effect_without_a_token_marks_it_done(
    subject: &dyn UowSubject,
) -> ContractResult {
    let aggregate = AggregateRef::incident(IncidentId::new());
    let effect = wake(&aggregate, 1);
    subject
        .uow()
        .commit(Transaction::new().effect(effect.clone()))
        .await?;
    subject
        .uow()
        .commit(Transaction::new().complete(EffectCompletion {
            id: effect.id,
            claim_token: None,
            at: at(4),
        }))
        .await?;
    let record = subject
        .effects()
        .effect(effect.id)
        .await?
        .ok_or_else(|| super::super::ContractError::Setup("effect missing".to_owned()))?;
    assert_eq!(record.status, "done");
    assert_eq!(record.completed_at, Some(at(4)));
    Ok(())
}

/// An effect with no allowed attempts is refused before any write.
pub async fn zero_max_attempts_is_refused(subject: &dyn UowSubject) -> ContractResult {
    let host = host("nas")?;
    let aggregate = AggregateRef::incident(IncidentId::new());
    let mut effect = wake(&aggregate, 1);
    effect.max_attempts = 0;
    let note = note(host.id);
    let note_id = note.id;
    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .effect(effect.clone())
                .write(StateWrite::Note {
                    note,
                    expected: Expected::New,
                }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Constraint(message)) if message == "max_attempts must be at least 1"
    ));
    assert_eq!(subject.versions().note(note_id).await?, None);
    assert_eq!(subject.effects().effect(effect.id).await?, None);
    Ok(())
}

/// An effect id is unique: reusing it is refused and leaves the first effect untouched.
pub async fn effect_id_must_be_unique(subject: &dyn UowSubject) -> ContractResult {
    let aggregate = AggregateRef::incident(IncidentId::new());
    let original = wake(&aggregate, 1);
    subject
        .uow()
        .commit(Transaction::new().effect(original.clone()))
        .await?;
    let before = subject
        .effects()
        .effect(original.id)
        .await?
        .ok_or_else(|| super::super::ContractError::Setup("effect missing".to_owned()))?;

    let mut competing = wake(&aggregate, 2);
    competing.id = original.id;
    competing.kind = EffectKind::Notify;
    competing.max_attempts = 9;
    let failed = subject
        .uow()
        .commit(Transaction::new().effect(competing))
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Constraint(ref message)) if message == "effect id exists"
    ));
    assert_eq!(
        subject.effects().effect(original.id).await?,
        Some(before),
        "the existing effect is unchanged"
    );

    // Two effects with one fresh id: neither becomes visible, and neither does the note
    // written before them.
    let host = host("nas")?;
    let note = note(host.id);
    let note_id = note.id;
    let shared = EffectId::new();
    let mut first = wake(&aggregate, 3);
    first.id = shared;
    let mut second = wake(&aggregate, 4);
    second.id = shared;
    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Note {
                    note,
                    expected: Expected::New,
                })
                .effect(first)
                .effect(second),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Constraint(ref message)) if message == "effect id exists"
    ));
    assert_eq!(subject.effects().effect(shared).await?, None);
    assert_eq!(subject.versions().note(note_id).await?, None);
    Ok(())
}

/// Effects are listed in insertion order, not by id or creation time.
pub async fn effects_are_listed_in_insertion_order(subject: &dyn UowSubject) -> ContractResult {
    let aggregate = AggregateRef::incident(IncidentId::new());
    // Id order is 1, 2, 3; creation times are 20, 10, 30; insertion is 3, 1, 2.
    let third = fixed_wake(3, &aggregate, 20);
    let first = fixed_wake(1, &aggregate, 10);
    let second = fixed_wake(2, &aggregate, 30);
    subject
        .uow()
        .commit(
            Transaction::new()
                .effect(third.clone())
                .effect(first.clone()),
        )
        .await?;
    subject
        .uow()
        .commit(Transaction::new().effect(second.clone()))
        .await?;

    let listed = subject
        .effects()
        .effects_with_status(EffectStatus::Pending, 10)
        .await?;
    assert_eq!(ids(&listed), vec![third.id, first.id, second.id]);

    let limited = subject
        .effects()
        .effects_with_status(EffectStatus::Pending, 2)
        .await?;
    assert_eq!(ids(&limited), vec![third.id, first.id]);
    Ok(())
}
