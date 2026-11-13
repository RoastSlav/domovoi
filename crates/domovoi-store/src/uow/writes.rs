//! Applying the typed state writes of a transaction on one SQLite connection.

use domovoi_core::ports::{
    ActionGuard, AggregateRef, CommitError, CommitReceipt, Expectation, Expected, JournalDraft,
    Observed, StateWrite,
};
use sqlx::SqliteConnection;

use crate::codec::uint;
use crate::repos::{
    DiscardOutcome, discard_pending_incident, insert_action, insert_incident,
    insert_incident_event, insert_note, insert_signal_and_link, update_action_from,
    update_incident, update_note,
};

use super::{backend, map_store};

/// Stores the version of an incident, or `None` when it does not exist.
async fn incident_version(
    conn: &mut SqliteConnection,
    id: domovoi_core::IncidentId,
) -> Result<Option<u64>, CommitError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT version FROM incidents WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(row.map(|(version,)| uint(version)))
}

/// Stores the version of a note, or `None` when it does not exist.
async fn note_version(
    conn: &mut SqliteConnection,
    id: domovoi_core::NoteId,
) -> Result<Option<u64>, CommitError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT version FROM notes WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(row.map(|(version,)| uint(version)))
}

/// The stored status name of an action, or `None` when it does not exist.
async fn action_status(
    conn: &mut SqliteConnection,
    id: domovoi_core::ActionId,
) -> Result<Option<String>, CommitError> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT json_extract(status, '$.type') FROM actions WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    Ok(row.map(|(status,)| status))
}

/// Refuses a second active incident for the same fingerprint.
async fn ensure_one_active(
    conn: &mut SqliteConnection,
    incident: &domovoi_core::Incident,
) -> Result<(), CommitError> {
    if incident.status() == domovoi_core::IncidentStatus::Resolved {
        return Ok(());
    }
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM incidents WHERE fingerprint = ? AND status <> 'resolved' AND id <> ?",
    )
    .bind(crate::codec::name(&incident.fingerprint()).map_err(map_store)?)
    .bind(incident.id().to_string())
    .fetch_optional(&mut *conn)
    .await
    .map_err(backend)?;
    if row.is_some() {
        return Err(CommitError::Constraint(
            "an active incident with this fingerprint exists".to_owned(),
        ));
    }
    Ok(())
}

/// Refuses a chat token hash another action already uses.
async fn ensure_token_unused(
    conn: &mut SqliteConnection,
    action: &domovoi_core::ActionRequest,
) -> Result<(), CommitError> {
    let Some(hash) = action.chat_token_hash() else {
        return Ok(());
    };
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM actions WHERE chat_token_hash = ? AND id <> ?")
            .bind(&hash[..])
            .bind(action.id().to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    if row.is_some() {
        return Err(CommitError::Constraint(
            "chat token already used".to_owned(),
        ));
    }
    Ok(())
}

/// Applies one state write. Mirrored journal rows are queued in write order.
pub(super) async fn apply(
    conn: &mut SqliteConnection,
    write: &StateWrite,
    receipt: &mut CommitReceipt,
    mirrors: &mut Vec<(JournalDraft, u64)>,
) -> Result<(), CommitError> {
    match write {
        StateWrite::Signal { signal, incident } => insert_signal_and_link(conn, signal, *incident)
            .await
            .map_err(map_store),
        StateWrite::Incident { incident, expected } => {
            apply_incident(conn, incident, *expected, receipt).await
        }
        StateWrite::IncidentEvent(entry) => {
            let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM incidents WHERE id = ?")
                .bind(entry.incident.to_string())
                .fetch_optional(&mut *conn)
                .await
                .map_err(backend)?;
            if exists.is_none() {
                return Err(CommitError::Constraint("unknown incident".to_owned()));
            }
            let seq = insert_incident_event(&mut *conn, entry)
                .await
                .map_err(map_store)?;
            mirrors.push((domovoi_core::ports::timeline_journal(entry), seq));
            Ok(())
        }
        StateWrite::DiscardPendingIncident { id, expected } => {
            apply_discard(conn, *id, *expected).await
        }
        StateWrite::Action { action, expected } => apply_action(conn, action, expected).await,
        StateWrite::Note { note, expected } => apply_note(conn, note, *expected, receipt).await,
    }
}

async fn apply_incident(
    conn: &mut SqliteConnection,
    incident: &domovoi_core::Incident,
    expected: Expected,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::incident(incident.id());
    let stored = incident_version(conn, incident.id()).await?;
    match (expected, stored) {
        (Expected::New, Some(version)) => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::New,
            found: Observed::Version(version),
        }),
        (Expected::New, None) => {
            ensure_one_active(conn, incident).await?;
            insert_incident(&mut *conn, incident)
                .await
                .map_err(map_store)?;
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        (Expected::Version(n), None) => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(n),
            found: Observed::Missing,
        }),
        (Expected::Version(n), Some(version)) if version != n => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(n),
            found: Observed::Version(version),
        }),
        (Expected::Version(n), Some(_)) => {
            ensure_one_active(conn, incident).await?;
            update_incident(&mut *conn, incident, Some(n))
                .await
                .map_err(map_store)?;
            receipt.versions.push((aggregate, n + 1));
            Ok(())
        }
    }
}

async fn apply_discard(
    conn: &mut SqliteConnection,
    id: domovoi_core::IncidentId,
    expected: u64,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::incident(id);
    match incident_version(conn, id).await? {
        None => {
            return Err(CommitError::Conflict {
                aggregate,
                expected: Expectation::Version(expected),
                found: Observed::Missing,
            });
        }
        Some(version) if version != expected => {
            return Err(CommitError::Conflict {
                aggregate,
                expected: Expectation::Version(expected),
                found: Observed::Version(version),
            });
        }
        Some(_) => {}
    }
    match discard_pending_incident(conn, id)
        .await
        .map_err(map_store)?
    {
        DiscardOutcome::Discarded => Ok(()),
        DiscardOutcome::Missing => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(expected),
            found: Observed::Missing,
        }),
        DiscardOutcome::NotPending => Err(CommitError::Constraint(
            "only pending incidents are discarded".to_owned(),
        )),
    }
}

async fn apply_action(
    conn: &mut SqliteConnection,
    action: &domovoi_core::ActionRequest,
    expected: &ActionGuard,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::action(action.id());
    let stored = action_status(conn, action.id()).await?;
    match (expected, stored) {
        (ActionGuard::New, Some(status)) => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::New,
            found: Observed::Status(status),
        }),
        (ActionGuard::New, None) => {
            ensure_token_unused(conn, action).await?;
            insert_action(&mut *conn, action).await.map_err(map_store)
        }
        (ActionGuard::Status(n), None) => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Status(n.clone()),
            found: Observed::Missing,
        }),
        (ActionGuard::Status(n), Some(status)) if status != *n => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Status(n.clone()),
            found: Observed::Status(status),
        }),
        (ActionGuard::Status(n), Some(_)) => {
            ensure_token_unused(conn, action).await?;
            let done = update_action_from(&mut *conn, action, n)
                .await
                .map_err(map_store)?;
            if done == 0 {
                let found = action_status(conn, action.id()).await?;
                return Err(CommitError::Conflict {
                    aggregate,
                    expected: Expectation::Status(n.clone()),
                    found: match found {
                        Some(status) => Observed::Status(status),
                        None => Observed::Missing,
                    },
                });
            }
            Ok(())
        }
    }
}

async fn apply_note(
    conn: &mut SqliteConnection,
    note: &domovoi_core::Note,
    expected: Expected,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::note(note.id);
    let stored = note_version(conn, note.id).await?;
    match (expected, stored) {
        (Expected::New, Some(version)) => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::New,
            found: Observed::Version(version),
        }),
        (Expected::New, None) => {
            insert_note(&mut *conn, note).await.map_err(map_store)?;
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        (Expected::Version(n), None) => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(n),
            found: Observed::Missing,
        }),
        (Expected::Version(n), Some(version)) if version != n => Err(CommitError::Conflict {
            aggregate,
            expected: Expectation::Version(n),
            found: Observed::Version(version),
        }),
        (Expected::Version(n), Some(_)) => {
            update_note(&mut *conn, note, Some(n))
                .await
                .map_err(map_store)?;
            receipt.versions.push((aggregate, n + 1));
            Ok(())
        }
    }
}

/// Rejects an effect draft before anything is written.
pub(super) fn validate_effect(draft: &domovoi_core::ports::EffectDraft) -> Result<(), CommitError> {
    if draft.max_attempts == 0 {
        return Err(CommitError::Constraint(
            "max_attempts must be at least 1".to_owned(),
        ));
    }
    Ok(())
}
