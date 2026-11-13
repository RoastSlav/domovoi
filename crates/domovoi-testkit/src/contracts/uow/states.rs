//! Unit-of-work contracts about state writes and conflicts.

use domovoi_core::ports::{
    ActionGuard, AggregateRef, AuditDraft, AuditEvent, AuditFilter, CommitError, EffectDraft,
    EffectKind, Expectation, Expected, IncidentTimelineEntry, JournalDraft, JournalKind, Observed,
    Page, StateWrite, Transaction,
};
use domovoi_core::{EffectId, IncidentId, Note, NoteId, Risk, UserId};
use serde_json::json;

use super::super::ContractResult;
use super::UowSubject;
use crate::build::{T0, action, at, host, incident, open_incident, signal};

fn audit(at_secs: i64, subject: String) -> AuditDraft {
    AuditDraft::new(AuditEvent {
        at: at(at_secs),
        actor: "system:test".to_owned(),
        kind: "test.write".to_owned(),
        subject,
        detail: serde_json::Value::Null,
    })
}

fn journal(aggregate: &AggregateRef, at_secs: i64) -> JournalDraft {
    JournalDraft {
        aggregate: aggregate.clone(),
        kind: JournalKind::ActionTransition,
        payload_v: 1,
        payload: json!({"to": "approved"}),
        actor: "user:1".to_owned(),
        occurred_at: at(at_secs),
    }
}

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

/// Commits state, journal, audit and an effect in one transaction.
pub async fn commit_applies_state_journal_audit_and_effects_together(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    let firing = signal(host.id, "disk.smart", Some("/dev/sda"))?;
    let entry = IncidentTimelineEntry {
        incident: incident.id(),
        at: at(5),
        kind: "status.changed".to_owned(),
        summary: "opened".to_owned(),
        detail: json!({"to": "open"}),
    };
    let aggregate = AggregateRef::incident(incident.id());
    let explicit = journal(&aggregate, 6);
    let effect = wake(&aggregate, 8);

    let tx = Transaction::new()
        .write(StateWrite::Incident {
            incident: incident.clone(),
            expected: Expected::New,
        })
        .write(StateWrite::Signal {
            signal: firing.clone(),
            incident: Some(incident.id()),
        })
        .write(StateWrite::IncidentEvent(entry.clone()))
        .journal(explicit)
        .audit(audit(7, format!("incident:{}", incident.id())))
        .effect(effect.clone());

    let receipt = subject.uow().commit(tx).await?;
    assert_eq!(receipt.journal_seqs.len(), 2, "mirror then explicit row");
    assert!(receipt.journal_seqs[0] < receipt.journal_seqs[1]);
    assert_eq!(receipt.audit_seqs, vec![1]);
    assert_eq!(receipt.versions, vec![(aggregate.clone(), 1)]);

    let stored = subject.versions().incident(incident.id()).await?;
    assert_eq!(stored.map(|v| v.version), Some(1));
    assert_eq!(
        subject.incidents().timeline(incident.id()).await?,
        vec![entry]
    );
    assert_eq!(
        subject
            .signals()
            .list_for_incident(incident.id(), 10)
            .await?
            .len(),
        1
    );

    let entries = subject.journal().entries_after(0, 10).await?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].seq, receipt.journal_seqs[0]);
    assert_eq!(entries[0].kind, "incident.timeline");
    assert_eq!(entries[1].seq, receipt.journal_seqs[1]);
    assert_eq!(entries[1].kind, "action.transition");

    let audit_rows = subject
        .audit()
        .list(&AuditFilter::default(), &Page::first(10))
        .await?;
    assert_eq!(audit_rows.items.len(), 1);

    let record =
        subject.effects().effect(effect.id).await?.ok_or_else(|| {
            super::super::ContractError::Setup("the effect is missing".to_owned())
        })?;
    assert_eq!(record.status, "pending");
    assert_eq!(record.attempt, 0);
    assert_eq!(record.claim_generation, 0);
    Ok(())
}

/// An empty transaction changes nothing.
pub async fn empty_transaction_commits_nothing(subject: &dyn UowSubject) -> ContractResult {
    let receipt = subject.uow().commit(Transaction::new()).await?;
    assert_eq!(receipt, Default::default());
    assert_eq!(subject.journal().head_seq().await?, 0);
    Ok(())
}

/// A stale incident version rolls the whole transaction back.
pub async fn stale_incident_version_conflicts_and_changes_nothing(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    subject.incidents().insert(&incident).await?;
    let aggregate = AggregateRef::incident(incident.id());

    let receipt = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Incident {
            incident: incident.clone(),
            expected: Expected::Version(1),
        }))
        .await?;
    assert_eq!(receipt.versions, vec![(aggregate.clone(), 2)]);

    let before_body = subject.incidents().get(incident.id()).await?;
    let before_audit = subject
        .audit()
        .list(&AuditFilter::default(), &Page::first(50))
        .await?;
    let before_head = subject.journal().head_seq().await?;

    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Incident {
                    incident: incident.clone(),
                    expected: Expected::Version(1),
                })
                .journal(journal(&aggregate, 9))
                .audit(audit(9, format!("incident:{}", incident.id()))),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Conflict {
            expected: Expectation::Version(1),
            found: Observed::Version(2),
            ..
        })
    ));
    assert_eq!(subject.incidents().get(incident.id()).await?, before_body);
    assert_eq!(
        subject
            .audit()
            .list(&AuditFilter::default(), &Page::first(50))
            .await?,
        before_audit
    );
    assert_eq!(subject.journal().head_seq().await?, before_head);
    Ok(())
}

/// A new incident over an existing id conflicts.
pub async fn new_incident_over_existing_id_conflicts(subject: &dyn UowSubject) -> ContractResult {
    let host = host("nas")?;
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    subject.incidents().insert(&incident).await?;
    let failed = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Incident {
            incident: incident.clone(),
            expected: Expected::New,
        }))
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Conflict {
            expected: Expectation::New,
            found: Observed::Version(1),
            ..
        })
    ));
    Ok(())
}

/// A second active incident for one fingerprint is refused, and the first is rolled back with it.
pub async fn second_active_incident_for_a_fingerprint_is_refused_and_rolls_back(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let first = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    let second = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_ne!(first.id(), second.id());

    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Incident {
                    incident: first.clone(),
                    expected: Expected::New,
                })
                .write(StateWrite::Incident {
                    incident: second.clone(),
                    expected: Expected::New,
                }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Constraint(message))
            if message == "an active incident with this fingerprint exists"
    ));
    assert_eq!(subject.versions().incident(first.id()).await?, None);
    assert_eq!(subject.versions().incident(second.id()).await?, None);
    Ok(())
}

/// A later failure rolls back earlier writes, journal rows, audit rows and effects.
pub async fn later_failure_rolls_back_earlier_writes_journal_audit_and_effects(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let note = Note {
        id: NoteId::new(),
        host: Some(host.id),
        title: "Runbook".to_owned(),
        body_markdown: "Steps".to_owned(),
        tags: vec!["ops".to_owned()],
        updated_at: T0,
    };
    let note_id = note.id;
    let note_ref = AggregateRef::note(note_id);
    let missing = IncidentId::new();
    let ghost = incident(host.id, "load.high", None)?.with_id(missing);
    let effect = wake(&AggregateRef::incident(missing), 10);

    let before_head = subject.journal().head_seq().await?;
    let before_audit = subject
        .audit()
        .list(&AuditFilter::default(), &Page::first(50))
        .await?;

    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Note {
                    note,
                    expected: Expected::New,
                })
                .journal(journal(&note_ref, 9))
                .audit(audit(9, "note:x".to_owned()))
                .effect(effect.clone())
                .write(StateWrite::Incident {
                    incident: ghost,
                    expected: Expected::Version(7),
                }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Conflict {
            expected: Expectation::Version(7),
            found: Observed::Missing,
            ..
        })
    ));
    assert_eq!(subject.versions().note(note_id).await?, None);
    assert_eq!(subject.journal().head_seq().await?, before_head);
    assert_eq!(
        subject
            .audit()
            .list(&AuditFilter::default(), &Page::first(50))
            .await?,
        before_audit
    );
    assert_eq!(subject.effects().effect(effect.id).await?, None);
    Ok(())
}

/// Repository writes advance versions the same way commits do.
pub async fn repository_writes_advance_versions_like_commits(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    subject.incidents().insert(&incident).await?;
    assert_eq!(
        subject
            .versions()
            .incident(incident.id())
            .await?
            .map(|v| v.version),
        Some(1)
    );
    subject.incidents().update(&incident).await?;
    assert_eq!(
        subject
            .versions()
            .incident(incident.id())
            .await?
            .map(|v| v.version),
        Some(2)
    );
    let receipt = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Incident {
            incident: incident.clone(),
            expected: Expected::Version(2),
        }))
        .await?;
    assert_eq!(
        receipt.versions,
        vec![(AggregateRef::incident(incident.id()), 3)]
    );
    assert_eq!(
        subject
            .versions()
            .incident(incident.id())
            .await?
            .map(|v| v.version),
        Some(3)
    );

    let note = Note {
        id: NoteId::new(),
        host: Some(host.id),
        title: "Pool scrub".to_owned(),
        body_markdown: "Start a scrub".to_owned(),
        tags: Vec::new(),
        updated_at: T0,
    };
    subject.notes().upsert(&note).await?;
    subject.notes().upsert(&note).await?;
    assert_eq!(
        subject.versions().note(note.id).await?.map(|v| v.version),
        Some(2)
    );
    let receipt = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Note {
            note: note.clone(),
            expected: Expected::Version(2),
        }))
        .await?;
    assert_eq!(receipt.versions, vec![(AggregateRef::note(note.id), 3)]);
    Ok(())
}

/// An action guard refuses a status another writer already changed.
pub async fn action_guard_refuses_a_changed_status(subject: &dyn UowSubject) -> ContractResult {
    let host = host("nas")?;
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    let proposed = action(incident.id(), host.id, Risk::Write)?;
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Action {
            action: proposed.clone(),
            expected: ActionGuard::New,
        }))
        .await?;

    let mut approved = proposed.clone();
    approved
        .approve(crate::approvals::human(UserId::new()), T0)
        .map_err(|e| super::super::ContractError::Setup(e.to_string()))?;
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Action {
            action: approved.clone(),
            expected: ActionGuard::Status("proposed".to_owned()),
        }))
        .await?;

    let failed = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Action {
            action: approved,
            expected: ActionGuard::Status("proposed".to_owned()),
        }))
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Conflict {
            expected: Expectation::Status(ref name),
            found: Observed::Status(ref found),
            ..
        }) if name == "proposed" && found == "approved"
    ));

    let ghost = action(incident.id(), host.id, Risk::Read)?;
    let failed = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Action {
            action: ghost,
            expected: ActionGuard::Status("proposed".to_owned()),
        }))
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Conflict {
            found: Observed::Missing,
            ..
        })
    ));
    Ok(())
}

/// A chat token hash is unique across actions, even inside one transaction.
pub async fn chat_token_hash_must_be_unique_across_actions(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = host("nas")?;
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda"))?;
    let first = action(incident.id(), host.id, Risk::Write)?.with_chat_token_hash([9; 32]);
    let second = action(incident.id(), host.id, Risk::Read)?.with_chat_token_hash([9; 32]);
    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Action {
                    action: first.clone(),
                    expected: ActionGuard::New,
                })
                .write(StateWrite::Action {
                    action: second,
                    expected: ActionGuard::New,
                }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Constraint(message)) if message == "chat token already used"
    ));
    assert_eq!(subject.actions().get(first.id()).await?, None);
    Ok(())
}
