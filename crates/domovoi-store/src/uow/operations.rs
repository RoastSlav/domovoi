//! Applying the operation aggregate's writes on one SQLite connection.
//!
//! Every structural rule is checked in Rust before the statement runs, so a refusal is always
//! a typed [`CommitError`]. The triggers in the migration are the second line of defence for
//! anyone writing raw SQL.

use domovoi_core::ports::{
    AggregateRef, CommitError, CommitReceipt, Expectation, Expected, Observed, OperationWrite,
};
use domovoi_core::{
    IncidentId, Operation, OperationId, Step, StepEdge, StepId, StepOutcome, StepProgress,
};
use sqlx::SqliteConnection;

use crate::codec::{int, json, ms, uint};

use super::operation_executions::apply_execution;
use super::{backend, map_store};

/// The message every refused edge carries.
const EDGE_RULE: &str = "an edge must point to an earlier step of the same operation";

pub(super) fn refuse(message: &str) -> CommitError {
    CommitError::Constraint(message.to_owned())
}

pub(super) fn conflict(id: OperationId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::operation(id),
        expected,
        found,
    }
}

/// Applies one operation write.
pub(super) async fn apply(
    conn: &mut SqliteConnection,
    write: &OperationWrite,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    match write {
        OperationWrite::Operation {
            operation,
            expected,
        } => apply_operation(conn, operation, *expected, receipt).await,
        OperationWrite::Step(step) => insert_step(conn, step).await,
        OperationWrite::Progress { step, from, to } => {
            update_progress(conn, *step, *from, to).await
        }
        OperationWrite::Edge(edge) => insert_edge(conn, edge).await,
        OperationWrite::Execution {
            execution,
            expected,
        } => apply_execution(conn, execution, *expected).await,
        OperationWrite::IncidentLink {
            operation,
            incident,
        } => link_incident(conn, *operation, *incident).await,
    }
}

async fn operation_version(
    conn: &mut SqliteConnection,
    id: OperationId,
) -> Result<Option<u64>, CommitError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT version FROM operations WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(row.map(|(version,)| uint(version)))
}

async fn operation_exists(
    conn: &mut SqliteConnection,
    id: OperationId,
) -> Result<bool, CommitError> {
    Ok(operation_version(conn, id).await?.is_some())
}

async fn apply_operation(
    conn: &mut SqliteConnection,
    operation: &Operation,
    expected: Expected,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    operation
        .validate()
        .map_err(|e| CommitError::Constraint(e.to_string()))?;
    let stored = operation_version(conn, operation.id).await?;
    let aggregate = AggregateRef::operation(operation.id);
    match (expected, stored) {
        (Expected::New, Some(version)) => Err(conflict(
            operation.id,
            Expectation::New,
            Observed::Version(version),
        )),
        (Expected::New, None) => {
            insert_operation(conn, operation).await?;
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        (Expected::Version(n), None) => Err(conflict(
            operation.id,
            Expectation::Version(n),
            Observed::Missing,
        )),
        (Expected::Version(n), Some(version)) if version != n => Err(conflict(
            operation.id,
            Expectation::Version(n),
            Observed::Version(version),
        )),
        (Expected::Version(n), Some(_)) => {
            update_operation(conn, operation, n).await?;
            receipt.versions.push((aggregate, n + 1));
            Ok(())
        }
    }
}

/// The columns of an operation row after `id`, encoded.
struct OperationColumns {
    kind: &'static str,
    status: &'static str,
    pause_reason: Option<String>,
    budgets: String,
    pinned: String,
    parent: Option<String>,
}

fn encode_operation(operation: &Operation) -> Result<OperationColumns, CommitError> {
    Ok(OperationColumns {
        kind: operation.kind.as_str(),
        status: operation.status.name(),
        pause_reason: operation
            .status
            .pause_reason()
            .map(json)
            .transpose()
            .map_err(map_store)?,
        budgets: json(&operation.budgets).map_err(map_store)?,
        pinned: json(&operation.pinned).map_err(map_store)?,
        parent: operation.parent.map(|p| p.to_string()),
    })
}

async fn insert_operation(
    conn: &mut SqliteConnection,
    operation: &Operation,
) -> Result<(), CommitError> {
    const SQL: &str = "INSERT INTO operations (id, kind, goal, status, pause_reason, cycle, \
        budgets, pinned, parent_id, created_at, started_at, ended_at, updated_at) \
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
    let c = encode_operation(operation)?;
    sqlx::query(SQL)
        .bind(operation.id.to_string())
        .bind(c.kind)
        .bind(&operation.goal)
        .bind(c.status)
        .bind(c.pause_reason)
        .bind(i64::from(operation.cycle))
        .bind(c.budgets)
        .bind(c.pinned)
        .bind(c.parent)
        .bind(ms(operation.created_at))
        .bind(operation.started_at.map(ms))
        .bind(operation.ended_at.map(ms))
        .bind(ms(operation.updated_at))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn update_operation(
    conn: &mut SqliteConnection,
    operation: &Operation,
    version: u64,
) -> Result<(), CommitError> {
    const SQL: &str = "UPDATE operations SET kind = ?, goal = ?, status = ?, pause_reason = ?, \
        cycle = ?, budgets = ?, pinned = ?, parent_id = ?, created_at = ?, started_at = ?, \
        ended_at = ?, updated_at = ?, version = version + 1 WHERE id = ? AND version = ?";
    let c = encode_operation(operation)?;
    sqlx::query(SQL)
        .bind(c.kind)
        .bind(&operation.goal)
        .bind(c.status)
        .bind(c.pause_reason)
        .bind(i64::from(operation.cycle))
        .bind(c.budgets)
        .bind(c.pinned)
        .bind(c.parent)
        .bind(ms(operation.created_at))
        .bind(operation.started_at.map(ms))
        .bind(operation.ended_at.map(ms))
        .bind(ms(operation.updated_at))
        .bind(operation.id.to_string())
        .bind(int(version))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

/// The operation and ordinal of a stored step.
async fn step_place(
    conn: &mut SqliteConnection,
    id: StepId,
) -> Result<Option<(String, u32)>, CommitError> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT operation_id, ordinal FROM steps WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    Ok(row.map(|(operation, ordinal)| (operation, u32::try_from(ordinal).unwrap_or(u32::MAX))))
}

async fn insert_step(conn: &mut SqliteConnection, step: &Step) -> Result<(), CommitError> {
    const SQL: &str = "INSERT INTO steps (id, operation_id, ordinal, kind, phase, \
        parent_step_id, cycle, intent, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)";
    step.validate()
        .map_err(|e| CommitError::Constraint(e.to_string()))?;
    if !operation_exists(conn, step.operation).await? {
        return Err(refuse("unknown operation"));
    }
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM steps WHERE operation_id = ?")
        .bind(step.operation.to_string())
        .fetch_one(&mut *conn)
        .await
        .map_err(backend)?;
    if i64::from(step.ordinal) != count {
        return Err(refuse(
            "step ordinals are dense: a new step takes the next one",
        ));
    }
    if let Some(parent) = step.parent {
        let ok = match step_place(conn, parent).await? {
            Some((operation, ordinal)) => {
                operation == step.operation.to_string() && ordinal < step.ordinal
            }
            None => false,
        };
        if !ok {
            return Err(refuse(
                "a step's parent must be an earlier step of the same operation",
            ));
        }
    }
    sqlx::query(SQL)
        .bind(step.id.to_string())
        .bind(step.operation.to_string())
        .bind(i64::from(step.ordinal))
        .bind(step.kind.as_str())
        .bind(&step.phase)
        .bind(step.parent.map(|p| p.to_string()))
        .bind(i64::from(step.cycle))
        .bind(json(&step.intent).map_err(map_store)?)
        .bind(ms(step.created_at))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn update_progress(
    conn: &mut SqliteConnection,
    step: StepId,
    from: StepOutcome,
    to: &StepProgress,
) -> Result<(), CommitError> {
    const SQL: &str = "UPDATE steps SET status = ?, started_at = ?, ended_at = ?, summary = ?, \
        reason = ?, output = ? WHERE id = ? AND status = ?";
    to.validate()
        .map_err(|e| CommitError::Constraint(e.to_string()))?;
    let done = sqlx::query(SQL)
        .bind(to.outcome.as_str())
        .bind(to.started_at.map(ms))
        .bind(to.ended_at.map(ms))
        .bind(&to.summary)
        .bind(&to.reason)
        .bind(&to.output)
        .bind(step.to_string())
        .bind(from.as_str())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    if done.rows_affected() > 0 {
        return Ok(());
    }
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT operation_id, status FROM steps WHERE id = ?")
            .bind(step.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    let Some((operation, status)) = row else {
        return Err(refuse("unknown step"));
    };
    let operation: OperationId = operation
        .parse()
        .map_err(|_| CommitError::Backend("stored operation id is unreadable".to_owned()))?;
    Err(conflict(
        operation,
        Expectation::Status(from.as_str().to_owned()),
        Observed::Status(status),
    ))
}

async fn insert_edge(conn: &mut SqliteConnection, edge: &StepEdge) -> Result<(), CommitError> {
    const SQL: &str =
        "INSERT INTO step_edges (operation_id, from_step, to_step, kind) VALUES (?, ?, ?, ?)";
    let (Some(from), Some(to)) = (
        step_place(conn, edge.from).await?,
        step_place(conn, edge.to).await?,
    ) else {
        return Err(refuse("unknown step"));
    };
    let operation = edge.operation.to_string();
    if from.0 != operation || to.0 != operation || to.1 >= from.1 {
        return Err(refuse(EDGE_RULE));
    }
    sqlx::query(SQL)
        .bind(operation)
        .bind(edge.from.to_string())
        .bind(edge.to.to_string())
        .bind(edge.kind.as_str())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn link_incident(
    conn: &mut SqliteConnection,
    operation: OperationId,
    incident: IncidentId,
) -> Result<(), CommitError> {
    // The link carries no time of its own: it takes the operation's, which the same
    // transaction has just written for a new operation.
    const SQL: &str = "INSERT OR IGNORE INTO operation_incidents \
        (operation_id, incident_id, linked_at) SELECT id, ?, updated_at FROM operations WHERE id = ?";
    if !operation_exists(conn, operation).await? {
        return Err(refuse("unknown operation"));
    }
    sqlx::query(SQL)
        .bind(incident.to_string())
        .bind(operation.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}
