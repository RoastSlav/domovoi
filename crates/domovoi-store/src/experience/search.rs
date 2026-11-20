//! Two-pass full-text search over summarised operations.

use domovoi_core::OperationId;
use domovoi_core::experience::{
    ExperienceHit, ExperienceQuery, ExperienceScope, MatchPass, query_words,
};
use domovoi_core::ports::StoreError;

use super::rows::load_summaries;
use crate::codec::parse;
use crate::db::Store;
use crate::error::db;
use crate::fts::{match_all, match_any};

/// The scope condition over alias `r`, with the host bound at parameter `host`. A null host
/// means every host; otherwise the operation's host set must be exactly that host.
pub(super) fn scope_clause(host: u8) -> String {
    format!(
        "(?{host} IS NULL OR (EXISTS (SELECT 1 FROM operation_resolution_hosts x \
         WHERE x.operation_id = r.operation_id AND x.host_id = ?{host}) \
         AND NOT EXISTS (SELECT 1 FROM operation_resolution_hosts y \
         WHERE y.operation_id = r.operation_id AND y.host_id <> ?{host})))"
    )
}

fn host_bind(scope: &ExperienceScope) -> Option<String> {
    match scope {
        ExperienceScope::Host(host) => Some(host.to_string()),
        ExperienceScope::Global => None,
    }
}

/// Matching ids with snippets, best first.
async fn matches(
    store: &Store,
    expression: &str,
    host: Option<&str>,
    exclude: Option<&str>,
    limit: usize,
) -> Result<Vec<(String, Option<String>)>, StoreError> {
    let sql = format!(
        "SELECT r.operation_id, \
         snippet(operation_resolutions_fts, -1, '', '', '…', 16) AS snippet, \
         -bm25(operation_resolutions_fts, 8.0, 6.0, 4.0, 4.0, 3.0, 3.0, 1.0) AS score \
         FROM operation_resolutions_fts \
         JOIN operation_resolutions r ON r.rowid = operation_resolutions_fts.rowid \
         WHERE operation_resolutions_fts MATCH ?1 AND {} AND (?3 IS NULL OR r.operation_id <> ?3) \
         ORDER BY score DESC, r.ended_at DESC, r.operation_id DESC LIMIT ?4",
        scope_clause(2)
    );
    let rows: Vec<(String, Option<String>, f64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(expression)
        .bind(host)
        .bind(exclude)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&store.read)
        .await
        .map_err(db)?;
    Ok(rows
        .into_iter()
        .map(|(id, snippet, _)| (id, snippet))
        .collect())
}

/// The newest operations of the scope.
async fn newest(
    store: &Store,
    host: Option<&str>,
    exclude: Option<&str>,
    limit: usize,
) -> Result<Vec<String>, StoreError> {
    let sql = format!(
        "SELECT r.operation_id FROM operation_resolutions r \
         WHERE {} AND (?2 IS NULL OR r.operation_id <> ?2) \
         ORDER BY r.ended_at DESC, r.operation_id DESC LIMIT ?3",
        scope_clause(1)
    );
    let rows: Vec<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(host)
        .bind(exclude)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&store.read)
        .await
        .map_err(db)?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub(super) async fn search(
    store: &Store,
    query: &ExperienceQuery,
) -> Result<Vec<ExperienceHit>, StoreError> {
    let limit = query.limit();
    let host = host_bind(&query.scope);
    let exclude = query.exclude.map(|id| id.to_string());
    let words = query_words(&query.text).join(" ");
    let mut found: Vec<(String, MatchPass, Option<String>)> = Vec::new();
    if let (Some(all), Some(any)) = (match_all(&words), match_any(&words)) {
        for (id, snippet) in
            matches(store, &all, host.as_deref(), exclude.as_deref(), limit).await?
        {
            found.push((id, MatchPass::AllWords, snippet));
        }
        if found.len() < limit {
            let wanted = limit + found.len();
            for (id, snippet) in
                matches(store, &any, host.as_deref(), exclude.as_deref(), wanted).await?
            {
                if found.len() < limit && !found.iter().any(|(seen, ..)| *seen == id) {
                    found.push((id, MatchPass::AnyWord, snippet));
                }
            }
        }
    } else {
        for id in newest(store, host.as_deref(), exclude.as_deref(), limit).await? {
            found.push((id, MatchPass::AllWords, None));
        }
    }
    let ids = found
        .iter()
        .map(|(id, ..)| parse::<OperationId>(id, "operation id"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut summaries = load_summaries(&store.read, &ids).await?;
    let mut hits = Vec::new();
    for ((_, pass, snippet), id) in found.into_iter().zip(ids) {
        if let Some(summary) = summaries.remove(&id) {
            hits.push(ExperienceHit {
                summary,
                pass,
                snippet: snippet.filter(|s| !s.is_empty()),
            });
        }
    }
    Ok(hits)
}
