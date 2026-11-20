//! Row types and shared queries for summary revisions.

use std::collections::BTreeMap;

use domovoi_core::OperationId;
use domovoi_core::experience::{ResolutionSummary, SummaryField, SummaryRevision, SummarySource};
use domovoi_core::ports::StoreError;
use sqlx::SqlitePool;

use crate::codec::{from_json, from_ms, parse};
use crate::error::{corrupt, db};

/// One stored revision.
#[derive(sqlx::FromRow)]
pub(super) struct RevisionRow {
    operation_id: String,
    field: String,
    revision: i64,
    value: String,
    source: String,
    actor: String,
    created_at: i64,
}

impl RevisionRow {
    pub(super) fn into_revision(self) -> Result<SummaryRevision, StoreError> {
        Ok(SummaryRevision {
            operation: parse(&self.operation_id, "summary operation id")?,
            field: SummaryField::parse(&self.field)
                .ok_or_else(|| corrupt("summary field", &self.field))?,
            revision: u32::try_from(self.revision).map_err(|e| corrupt("summary revision", e))?,
            value: from_json(&self.value, "summary value")?,
            source: SummarySource::parse(&self.source)
                .ok_or_else(|| corrupt("summary source", &self.source))?,
            actor: self.actor,
            created_at: from_ms(self.created_at)?,
        })
    }
}

/// The SQL listing the newest revision of each field where `filter` holds, in field order
/// within an operation. `filter` must be a static fragment over the alias `f`.
pub(super) fn current_sql(filter: &str) -> String {
    format!(
        "SELECT f.operation_id, f.field, f.revision, f.value, f.source, f.actor, f.created_at \
         FROM resolution_summary_fields f WHERE {filter} AND f.revision = \
         (SELECT max(m.revision) FROM resolution_summary_fields m \
          WHERE m.operation_id = f.operation_id AND m.field = f.field) \
         ORDER BY f.operation_id, CASE f.field {} END",
        field_order()
    )
}

/// A `WHEN 'outcome' THEN 0 ...` list that sorts fields in [`SummaryField::ALL`] order.
pub(super) fn field_order() -> String {
    SummaryField::ALL
        .iter()
        .enumerate()
        .map(|(i, f)| format!("WHEN '{}' THEN {i}", f.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// Loads the current summary of each id that has one.
pub(super) async fn load_summaries(
    pool: &SqlitePool,
    ids: &[OperationId],
) -> Result<BTreeMap<OperationId, ResolutionSummary>, StoreError> {
    let mut out = BTreeMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let marks = placeholders(ids.len());
    let sql = current_sql(&format!("f.operation_id IN ({marks})"));
    let mut query = sqlx::query_as::<_, RevisionRow>(sqlx::AssertSqlSafe(sql));
    for id in ids {
        query = query.bind(id.to_string());
    }
    let rows = query.fetch_all(pool).await.map_err(db)?;
    let mut grouped: BTreeMap<OperationId, Vec<SummaryRevision>> = BTreeMap::new();
    for row in rows {
        let revision = row.into_revision()?;
        grouped
            .entry(revision.operation)
            .or_default()
            .push(revision);
    }
    let sql = format!("SELECT id, goal, ended_at FROM operations WHERE id IN ({marks})");
    let mut query = sqlx::query_as::<_, (String, String, Option<i64>)>(sqlx::AssertSqlSafe(sql));
    for id in ids {
        query = query.bind(id.to_string());
    }
    for (id, goal, ended) in query.fetch_all(pool).await.map_err(db)? {
        let id: OperationId = parse(&id, "operation id")?;
        let (Some(revisions), Some(ended)) = (grouped.get(&id), ended) else {
            continue;
        };
        if let Some(summary) = ResolutionSummary::fold(&goal, from_ms(ended)?, revisions)
            .map_err(|e| corrupt("summary", e))?
        {
            out.insert(id, summary);
        }
    }
    Ok(out)
}
