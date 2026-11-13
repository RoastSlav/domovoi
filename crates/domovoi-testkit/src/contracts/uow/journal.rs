//! Unit-of-work contracts about the journal, timeline mirroring and audit sequence.

use domovoi_core::IncidentId;
use domovoi_core::ports::{
    AggregateRef, AuditDraft, AuditEvent, CommitError, IncidentTimelineEntry, JournalDraft,
    JournalKind, StateWrite, Transaction,
};
use serde_json::json;

use super::super::ContractResult;
use super::UowSubject;
use crate::build::{T0, at, host, incident, open_incident, signal};

fn journal(aggregate: AggregateRef, at_secs: i64) -> JournalDraft {
    JournalDraft {
        aggregate,
        kind: JournalKind::ActionTransition,
        payload_v: 1,
        payload: json!({"to": "approved"}),
        actor: "user:1".to_owned(),
        occurred_at: at(at_secs),
    }
}

/// A timeline write is mirrored into the journal with its original time.
pub async fn incident_event_is_mirrored_into_the_journal_with_its_original_time(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let open = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    subject.incidents().insert(&open).await?;
    let entry = IncidentTimelineEntry {
        incident: open.id(),
        at: at(500),
        kind: "status.changed".to_owned(),
        summary: "resolved".to_owned(),
        detail: json!({"to": "resolved"}),
    };
    let receipt = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::IncidentEvent(entry.clone())))
        .await?;
    assert_eq!(receipt.journal_seqs.len(), 1);
    assert_eq!(
        subject.incidents().timeline(open.id()).await?,
        vec![entry.clone()]
    );

    let entries = subject
        .journal()
        .entries_for(&AggregateRef::incident(open.id()), 10)
        .await?;
    assert_eq!(entries.len(), 1);
    let row = &entries[0];
    assert_eq!(row.kind, "incident.timeline");
    assert_eq!(row.occurred_at, at(500));
    assert_eq!(row.actor, "system:incidents");
    assert_eq!(
        row.payload,
        json!({
            "kind": entry.kind,
            "summary": entry.summary,
            "detail": entry.detail,
        })
    );
    Ok(())
}

/// Journal sequence numbers follow commit order and filter by aggregate.
pub async fn journal_sequence_follows_commit_order(subject: &dyn UowSubject) -> ContractResult {
    let aggregate = AggregateRef::incident(IncidentId::new());
    let first = subject
        .uow()
        .commit(Transaction::new().journal(journal(aggregate.clone(), 1)))
        .await?;
    let second = subject
        .uow()
        .commit(Transaction::new().journal(journal(aggregate.clone(), 2)))
        .await?;
    let third = subject
        .uow()
        .commit(
            Transaction::new().journal(journal(AggregateRef::note(domovoi_core::NoteId::new()), 3)),
        )
        .await?;
    let first_seq = first.journal_seqs[0];
    assert!(first_seq < second.journal_seqs[0]);
    assert!(second.journal_seqs[0] < third.journal_seqs[0]);

    let later = subject.journal().entries_after(first_seq, 10).await?;
    assert_eq!(later.len(), 2);
    assert_eq!(later[0].seq, second.journal_seqs[0]);
    assert_eq!(subject.journal().head_seq().await?, third.journal_seqs[0]);
    assert_eq!(
        subject.journal().entries_for(&aggregate, 10).await?.len(),
        2
    );
    Ok(())
}

/// Discarding a pending incident removes its timeline and links, but keeps journal rows.
pub async fn discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let pending = incident(host.id, "disk.smart", Some("/dev/sda"))?;
    subject.incidents().insert(&pending).await?;
    let firing = signal(host.id, "disk.smart", Some("/dev/sda"))?;
    subject
        .signals()
        .insert(&firing, Some(pending.id()))
        .await?;
    let entry = IncidentTimelineEntry {
        incident: pending.id(),
        at: at(2),
        kind: "status.changed".to_owned(),
        summary: "opened".to_owned(),
        detail: serde_json::Value::Null,
    };
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::IncidentEvent(entry)))
        .await?;
    let head = subject.journal().head_seq().await?;

    let receipt = subject
        .uow()
        .commit(
            Transaction::new().write(StateWrite::DiscardPendingIncident {
                id: pending.id(),
                expected: 1,
            }),
        )
        .await?;
    assert!(receipt.versions.is_empty());
    assert_eq!(subject.versions().incident(pending.id()).await?, None);
    assert!(subject.incidents().timeline(pending.id()).await?.is_empty());
    assert!(
        subject
            .signals()
            .list_for_incident(pending.id(), 10)
            .await?
            .is_empty()
    );
    assert_eq!(subject.journal().head_seq().await?, head);

    let open = open_incident(host.id, "disk.smart", Some("/dev/sdb"))?;
    subject.incidents().insert(&open).await?;
    let failed = subject
        .uow()
        .commit(
            Transaction::new().write(StateWrite::DiscardPendingIncident {
                id: open.id(),
                expected: 1,
            }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Constraint(message))
            if message == "only pending incidents are discarded"
    ));
    Ok(())
}

/// An audit draft takes the next sequence number.
pub async fn audit_draft_is_appended_with_the_next_sequence_number(
    subject: &dyn UowSubject,
) -> ContractResult {
    let before = subject
        .audit()
        .append(&AuditEvent {
            at: T0,
            actor: "system:test".to_owned(),
            kind: "test.before".to_owned(),
            subject: "first".to_owned(),
            detail: serde_json::Value::Null,
        })
        .await?;
    let receipt = subject
        .uow()
        .commit(Transaction::new().audit(AuditDraft::new(AuditEvent {
            at: at(1),
            actor: "system:test".to_owned(),
            kind: "test.commit".to_owned(),
            subject: "second".to_owned(),
            detail: serde_json::Value::Null,
        })))
        .await?;
    assert_eq!(receipt.audit_seqs, vec![before + 1]);
    Ok(())
}
