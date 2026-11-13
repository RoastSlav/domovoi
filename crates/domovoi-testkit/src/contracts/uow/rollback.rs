//! A failure after the journal, audit and effect stages still rolls everything back.

use domovoi_core::ports::{
    AggregateRef, AuditDraft, AuditEvent, AuditFilter, ClaimToken, CommitError, EffectCompletion,
    EffectDraft, EffectKind, Expected, JournalDraft, JournalKind, Page, StateWrite, Transaction,
};
use domovoi_core::{EffectId, IncidentId, Note, NoteId};
use serde_json::json;

use super::super::{ContractError, ContractResult};
use super::{UowSubject, lease};
use crate::build::{T0, at, host};

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

/// A stale completion is refused after state, journal, audit and a new effect were applied,
/// and all of them roll back.
pub async fn failed_effect_completion_rolls_back_every_stage(
    subject: &dyn UowSubject,
) -> ContractResult {
    let aggregate = AggregateRef::incident(IncidentId::new());
    let claimed = wake(&aggregate, 1);
    subject
        .uow()
        .commit(Transaction::new().effect(claimed.clone()))
        .await?;
    let held_by = ClaimToken::new();
    subject.force_claim(claimed.id, held_by, lease(60)).await?;
    let before_effect = subject
        .effects()
        .effect(claimed.id)
        .await?
        .ok_or_else(|| ContractError::Setup("effect missing".to_owned()))?;
    assert_eq!(before_effect.status, "claimed");
    assert_eq!(before_effect.claim_token, Some(held_by));
    let before_head = subject.journal().head_seq().await?;
    let before_audit = subject
        .audit()
        .list(&AuditFilter::default(), &Page::first(50))
        .await?;

    let host = host("nas")?;
    let note = note(host.id);
    let note_id = note.id;
    let note_ref = AggregateRef::note(note_id);
    let new_effect = wake(&aggregate, 2);
    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Note {
                    note,
                    expected: Expected::New,
                })
                .journal(JournalDraft {
                    aggregate: note_ref,
                    kind: JournalKind::ActionTransition,
                    payload_v: 1,
                    payload: json!({"to": "approved"}),
                    actor: "user:1".to_owned(),
                    occurred_at: at(3),
                })
                .audit(AuditDraft::new(AuditEvent {
                    at: at(4),
                    actor: "system:test".to_owned(),
                    kind: "test.write".to_owned(),
                    subject: "note:x".to_owned(),
                    detail: serde_json::Value::Null,
                }))
                .effect(new_effect.clone())
                .complete(EffectCompletion {
                    id: claimed.id,
                    claim_token: Some(ClaimToken::new()),
                    at: at(5),
                }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::StaleEffect { effect }) if effect == claimed.id
    ));

    assert_eq!(subject.versions().note(note_id).await?, None);
    assert_eq!(subject.journal().head_seq().await?, before_head);
    assert!(
        subject
            .journal()
            .entries_after(before_head, 10)
            .await?
            .is_empty()
    );
    assert_eq!(
        subject
            .audit()
            .list(&AuditFilter::default(), &Page::first(50))
            .await?,
        before_audit
    );
    assert_eq!(subject.effects().effect(new_effect.id).await?, None);
    assert_eq!(
        subject.effects().effect(claimed.id).await?,
        Some(before_effect),
        "the claimed effect keeps its state and token"
    );
    Ok(())
}
