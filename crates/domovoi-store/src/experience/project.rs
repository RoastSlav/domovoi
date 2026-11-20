//! The derived projection: one row per summarised operation, the host and subject side tables
//! and, through triggers, the full-text index. Written only here, in the revision's transaction.

use domovoi_core::experience::{ResolutionSummary, SummaryField, contracts_text, subjects_text};
use domovoi_core::ports::{CommitError, SummaryFieldWrite};
use sqlx::SqliteConnection;

use super::rows::{RevisionRow, current_sql};
use crate::codec::{from_ms, ms};
use crate::uow::backend;

/// Rewrites the projection of `write.operation` from its newest revisions.
pub(super) async fn refresh(
    conn: &mut SqliteConnection,
    write: &SummaryFieldWrite,
) -> Result<(), CommitError> {
    let key = write.operation.to_string();
    let rows: Vec<RevisionRow> =
        sqlx::query_as(sqlx::AssertSqlSafe(current_sql("f.operation_id = ?")))
            .bind(&key)
            .fetch_all(&mut *conn)
            .await
            .map_err(backend)?;
    let current = rows
        .into_iter()
        .map(RevisionRow::into_revision)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CommitError::Backend(e.to_string()))?;
    let (goal, ended): (String, i64) =
        sqlx::query_as("SELECT goal, ended_at FROM operations WHERE id = ?")
            .bind(&key)
            .fetch_one(&mut *conn)
            .await
            .map_err(backend)?;
    let ended_at = from_ms(ended).map_err(|e| CommitError::Backend(e.to_string()))?;
    let summary = ResolutionSummary::fold(&goal, ended_at, &current)
        .map_err(|e| CommitError::Constraint(format!("bad summary value: {e}")))?
        .ok_or_else(|| {
            CommitError::Constraint("the first summary revision is the outcome".to_owned())
        })?;
    let text = |n: &Option<domovoi_core::experience::NarrativeField>| {
        n.as_ref().map(|f| f.text.clone()).unwrap_or_default()
    };
    sqlx::query(
        "INSERT INTO operation_resolutions \
         (operation_id, goal, outcome, verified, problem, cause, fix, subjects, contracts, tags, \
          ended_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT(operation_id) DO UPDATE SET \
           goal = excluded.goal, outcome = excluded.outcome, verified = excluded.verified, \
           problem = excluded.problem, cause = excluded.cause, fix = excluded.fix, \
           subjects = excluded.subjects, contracts = excluded.contracts, \
           tags = CASE WHEN ?13 THEN excluded.tags ELSE operation_resolutions.tags END, \
           ended_at = excluded.ended_at, updated_at = excluded.updated_at",
    )
    .bind(&key)
    .bind(&summary.goal)
    .bind(summary.outcome.as_str())
    .bind(i64::from(summary.verified))
    .bind(text(&summary.problem))
    .bind(text(&summary.cause))
    .bind(text(&summary.fix))
    .bind(subjects_text(&summary.subjects))
    .bind(contracts_text(&summary.contracts))
    .bind(write.tags.as_ref().map(|t| t.join(" ")).unwrap_or_default())
    .bind(ended)
    .bind(ms(write.at))
    .bind(write.tags.is_some())
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    match write.field {
        SummaryField::Hosts => {
            sqlx::query("DELETE FROM operation_resolution_hosts WHERE operation_id = ?")
                .bind(&key)
                .execute(&mut *conn)
                .await
                .map_err(backend)?;
            for host in &summary.hosts {
                sqlx::query(
                    "INSERT OR IGNORE INTO operation_resolution_hosts (operation_id, host_id) \
                     VALUES (?, ?)",
                )
                .bind(&key)
                .bind(host.to_string())
                .execute(&mut *conn)
                .await
                .map_err(backend)?;
            }
        }
        SummaryField::Subjects => {
            sqlx::query("DELETE FROM operation_resolution_subjects WHERE operation_id = ?")
                .bind(&key)
                .execute(&mut *conn)
                .await
                .map_err(backend)?;
            for subject in &summary.subjects {
                sqlx::query(
                    "INSERT OR IGNORE INTO operation_resolution_subjects \
                     (operation_id, host_id, check_key, target) VALUES (?, ?, ?, ?)",
                )
                .bind(&key)
                .bind(subject.host.to_string())
                .bind(subject.check.to_string())
                .bind(
                    subject
                        .target
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                )
                .execute(&mut *conn)
                .await
                .map_err(backend)?;
            }
        }
        _ => {}
    }
    Ok(())
}
