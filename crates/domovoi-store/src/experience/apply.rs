//! The `SummaryField` write: validate, append the revision, refresh the projection.

use domovoi_core::experience::SummaryField;
use domovoi_core::ports::{AggregateRef, CommitError, Expectation, Observed, SummaryFieldWrite};
use sqlx::SqliteConnection;

use super::project;
use crate::codec::{int, json, ms, uint};
use crate::uow::backend;

fn refuse(message: &str) -> CommitError {
    CommitError::Constraint(message.to_owned())
}

/// Applies one summary revision on the transaction's connection.
pub(crate) async fn apply(
    conn: &mut SqliteConnection,
    write: &SummaryFieldWrite,
) -> Result<(), CommitError> {
    if !write.source.may_write(write.field) {
        return Err(refuse("source cannot write this field"));
    }
    let key = write.operation.to_string();
    let finished: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT status, ended_at FROM operations WHERE id = ?")
            .bind(&key)
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    let is_finished = finished.is_some_and(|(status, ended)| {
        ended.is_some()
            && ["succeeded", "failed", "escalated", "cancelled"].contains(&status.as_str())
    });
    if !is_finished {
        return Err(refuse("a summary needs a finished operation"));
    }
    let (any,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM resolution_summary_fields WHERE operation_id = ?")
            .bind(&key)
            .fetch_one(&mut *conn)
            .await
            .map_err(backend)?;
    if any == 0 && write.field != SummaryField::Outcome {
        return Err(refuse("the first summary revision is the outcome"));
    }
    let (newest,): (i64,) = sqlx::query_as(
        "SELECT coalesce(max(revision), 0) FROM resolution_summary_fields \
         WHERE operation_id = ? AND field = ?",
    )
    .bind(&key)
    .bind(write.field.as_str())
    .fetch_one(&mut *conn)
    .await
    .map_err(backend)?;
    let newest = u32::try_from(uint(newest)).unwrap_or(u32::MAX);
    if newest != write.expected_revision {
        return Err(CommitError::Conflict {
            aggregate: AggregateRef::operation(write.operation),
            expected: Expectation::Version(u64::from(write.expected_revision)),
            found: Observed::Version(u64::from(newest)),
        });
    }
    if !write.field.accepts(&write.value) {
        return Err(refuse("bad summary value"));
    }
    let value = json(&write.value).map_err(|e| CommitError::Backend(e.to_string()))?;
    sqlx::query(
        "INSERT INTO resolution_summary_fields \
         (operation_id, field, revision, value, source, actor, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&key)
    .bind(write.field.as_str())
    .bind(int(u64::from(newest) + 1))
    .bind(value)
    .bind(write.source.as_str())
    .bind(&write.actor)
    .bind(ms(write.at))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    project::refresh(conn, write).await
}
