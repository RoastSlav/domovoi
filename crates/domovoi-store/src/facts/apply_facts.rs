//! Applying fact writes and path confirmations.

use domovoi_core::FactRecordId;
use domovoi_core::knowledge::{
    Fact, FactUpdate, KnowledgeStatus, PathConfirmation, Predicate, canonical_json, search_text,
};
use domovoi_core::ports::{AggregateRef, CommitError, CommitReceipt, Expectation, Observed};
use sqlx::SqliteConnection;

use crate::codec::{ms, uint};
use crate::uow::backend;

fn conflict(id: FactRecordId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::fact(id),
        expected,
        found,
    }
}

/// Inserts a new live fact at version 1.
pub(super) async fn insert(
    conn: &mut SqliteConnection,
    fact: &Fact,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let Some(value) = &fact.value else {
        return Err(CommitError::Constraint("a fact needs a value".to_owned()));
    };
    if !fact.status.is_live() {
        return Err(CommitError::Constraint(
            "a new fact must be live".to_owned(),
        ));
    }
    let existing: Option<(i64,)> = sqlx::query_as("SELECT version FROM facts WHERE id = ?")
        .bind(fact.id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    if let Some((version,)) = existing {
        return Err(conflict(
            fact.id,
            Expectation::New,
            Observed::Version(uint(version)),
        ));
    }
    sqlx::query(
        "INSERT INTO facts (id, subject_ref, host_id, predicate, value, search_text, live, \
         source, status, observed_at, valid_until, last_verified_at, supersedes_id, \
         evidence_ref, actor, created_at, retracted_at, retracted_by, content_purged_at, \
         version) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(fact.id.to_string())
    .bind(fact.subject.to_string())
    .bind(fact.subject.host().map(|h| h.to_string()))
    .bind(fact.predicate.as_str())
    .bind(canonical_json(value))
    .bind(search_text(value))
    .bind(fact.source.as_str())
    .bind(fact.status.as_str())
    .bind(ms(fact.observed_at))
    .bind(fact.valid_until.map(ms))
    .bind(fact.last_verified_at.map(ms))
    .bind(fact.supersedes.map(|s| s.to_string()))
    .bind(fact.evidence.as_ref().map(ToString::to_string))
    .bind(fact.actor.to_string())
    .bind(ms(fact.created_at))
    .bind(fact.retracted_at.map(ms))
    .bind(fact.retracted_by.as_ref().map(ToString::to_string))
    .bind(fact.content_purged_at.map(ms))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    receipt.versions.push((AggregateRef::fact(fact.id), 1));
    Ok(())
}

/// Changes the mutable columns of a live fact.
pub(super) async fn update(
    conn: &mut SqliteConnection,
    id: FactRecordId,
    expected: u64,
    update: &FactUpdate,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT version, status FROM facts WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    let Some((version, status)) = row else {
        return Err(conflict(
            id,
            Expectation::Version(expected),
            Observed::Missing,
        ));
    };
    let version = uint(version);
    if version != expected {
        return Err(conflict(
            id,
            Expectation::Version(expected),
            Observed::Version(version),
        ));
    }
    let stored = KnowledgeStatus::parse(&status)
        .ok_or_else(|| CommitError::Backend(format!("unknown stored fact status {status}")))?;
    if !stored.is_live() {
        return Err(CommitError::Constraint(format!("the fact is {status}")));
    }
    let retracting = update.status == Some(KnowledgeStatus::Retracted);
    if retracting != update.retraction.is_some() {
        return Err(CommitError::Constraint(
            "retraction and the retracted status go together".to_owned(),
        ));
    }
    let new_status = update.status.unwrap_or(stored);
    sqlx::query(
        "UPDATE facts SET status = ?, live = ?, observed_at = COALESCE(?, observed_at), \
         last_verified_at = COALESCE(?, last_verified_at), \
         retracted_at = COALESCE(?, retracted_at), retracted_by = COALESCE(?, retracted_by), \
         version = version + 1 WHERE id = ?",
    )
    .bind(new_status.as_str())
    .bind(i64::from(new_status.is_live()))
    .bind(update.observed_at.map(ms))
    .bind(update.last_verified_at.map(ms))
    .bind(update.retraction.as_ref().map(|(at, _)| ms(*at)))
    .bind(update.retraction.as_ref().map(|(_, by)| by.to_string()))
    .bind(id.to_string())
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    receipt
        .versions
        .push((AggregateRef::fact(id), expected + 1));
    Ok(())
}

/// Inserts or replaces the confirmation of a management-path fact.
pub(super) async fn confirm(
    conn: &mut SqliteConnection,
    confirmation: &PathConfirmation,
) -> Result<(), CommitError> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT predicate, host_id FROM facts WHERE id = ?")
            .bind(confirmation.fact.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    let ok = row.is_some_and(|(predicate, host)| {
        Predicate::parse(&predicate) == Some(Predicate::ManagementPath)
            && host.as_deref() == Some(confirmation.host.to_string().as_str())
    });
    if !ok {
        return Err(CommitError::Constraint(
            "only a management path of the same host can be confirmed".to_owned(),
        ));
    }
    sqlx::query(
        "INSERT OR REPLACE INTO management_path_confirmations \
         (fact_id, host_id, confirmed_by, confirmed_at, evidence) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(confirmation.fact.to_string())
    .bind(confirmation.host.to_string())
    .bind(confirmation.confirmed_by.to_string())
    .bind(ms(confirmation.confirmed_at))
    .bind(confirmation.evidence.as_str())
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}
