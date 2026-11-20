//! `KnowledgeRead` on the read pool.

use async_trait::async_trait;
use domovoi_core::knowledge::{
    Entity, Fact, PathConfirmation, Predicate, Relation, RelationKind, SubjectRef,
};
use domovoi_core::ports::{
    FactFilter, FactHit, KnowledgeRead, Page, Paged, RelationFilter, StoreError, Versioned,
};
use domovoi_core::{Actor, EntityId, FactRecordId, HostId, RelationId};

use super::rows::{EntityRow, FactRow, HitRow, RelationRow, fact_columns, relation_columns};
use crate::codec::{Cursor, fetch_limit, from_ms, paginate, parse};
use crate::db::Store;
use crate::error::db;
use crate::fts::match_all;

/// The most search candidates read before ranking.
const MAX_CANDIDATES: i64 = 200;

fn status_clause(filter: &FactFilter) -> String {
    match &filter.statuses {
        None => "live = 1".to_owned(),
        Some(list) if list.is_empty() => "0".to_owned(),
        Some(list) => {
            // Status names come from a closed enum, never from user text.
            let names: Vec<String> = list.iter().map(|s| format!("'{}'", s.as_str())).collect();
            format!("status IN ({})", names.join(", "))
        }
    }
}

#[async_trait]
impl KnowledgeRead for Store {
    async fn fact(&self, id: FactRecordId) -> Result<Option<Versioned<Fact>>, StoreError> {
        let sql = format!("SELECT {} FROM facts WHERE id = ?", fact_columns(""));
        let row: Option<FactRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(FactRow::into_fact).transpose()
    }

    async fn facts(&self, filter: &FactFilter, page: &Page) -> Result<Paged<Fact>, StoreError> {
        let sql = format!(
            "SELECT {} FROM facts WHERE (?1 IS NULL OR host_id = ?1) \
             AND (?2 IS NULL OR subject_ref = ?2) AND (?3 IS NULL OR predicate = ?3) \
             AND (?4 IS NULL OR source = ?4) \
             AND (?5 IS NULL OR created_at < ?5 OR (created_at = ?5 AND id < ?6)) \
             AND {} ORDER BY created_at DESC, id DESC LIMIT ?7",
            fact_columns(""),
            status_clause(filter)
        );
        let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<FactRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(filter.host.map(|h| h.to_string()))
            .bind(filter.subject.as_ref().map(ToString::to_string))
            .bind(filter.predicate.map(Predicate::as_str))
            .bind(filter.source.map(|s| s.as_str()))
            .bind(after_key)
            .bind(after_id)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let (rows, next_cursor) = paginate(rows, limit, |r| {
            let (key, id) = r.key();
            Cursor { key, id }
        });
        let items = rows
            .into_iter()
            .map(|r| r.into_fact().map(|v| v.value))
            .collect::<Result<_, _>>()?;
        Ok(Paged { items, next_cursor })
    }

    async fn live_facts(
        &self,
        subject: &SubjectRef,
        predicate: Predicate,
    ) -> Result<Vec<Versioned<Fact>>, StoreError> {
        let sql = format!(
            "SELECT {} FROM facts WHERE subject_ref = ? AND predicate = ? AND live = 1 \
             ORDER BY created_at, id",
            fact_columns("")
        );
        let rows: Vec<FactRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(subject.to_string())
            .bind(predicate.as_str())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(FactRow::into_fact).collect()
    }

    async fn all_live_facts(&self, limit: u32) -> Result<Vec<Versioned<Fact>>, StoreError> {
        let sql = format!(
            "SELECT {} FROM facts WHERE live = 1 ORDER BY created_at, id LIMIT ?",
            fact_columns("")
        );
        let rows: Vec<FactRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(i64::from(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(FactRow::into_fact).collect()
    }

    async fn search_facts(
        &self,
        query: &str,
        host: Option<HostId>,
        limit: u16,
    ) -> Result<Vec<FactHit>, StoreError> {
        let Some(fts) = match_all(query) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT {}, -bm25(facts_fts) AS score FROM facts_fts \
             JOIN facts f ON f.rowid = facts_fts.rowid \
             WHERE facts_fts MATCH ?1 AND f.live = 1 \
             AND (?2 IS NULL OR f.host_id IS NULL OR f.host_id = ?2) \
             ORDER BY score DESC, f.created_at DESC, f.id DESC LIMIT ?3",
            fact_columns("f.")
        );
        let rows: Vec<HitRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(fts)
            .bind(host.map(|h| h.to_string()))
            .bind(MAX_CANDIDATES)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let mut hits = rows
            .into_iter()
            .map(|row| {
                Ok(FactHit {
                    fact: row.fact.into_fact()?.value,
                    score: row.score,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        hits.truncate(usize::from(limit.clamp(1, Page::MAX_LIMIT)));
        Ok(hits)
    }

    async fn confirmation(
        &self,
        fact: FactRecordId,
    ) -> Result<Option<PathConfirmation>, StoreError> {
        let row: Option<(String, String, i64, String)> = sqlx::query_as(
            "SELECT host_id, confirmed_by, confirmed_at, evidence \
             FROM management_path_confirmations WHERE fact_id = ?",
        )
        .bind(fact.to_string())
        .fetch_optional(&self.read)
        .await
        .map_err(db)?;
        row.map(|(host, by, at, evidence)| {
            Ok(PathConfirmation {
                fact,
                host: parse(&host, "confirmation host")?,
                confirmed_by: parse::<Actor>(&by, "confirmation actor")?,
                confirmed_at: from_ms(at)?,
                evidence: domovoi_core::knowledge::CoverageEvidence::new(evidence)
                    .ok_or_else(|| crate::error::corrupt("confirmation evidence", "length"))?,
            })
        })
        .transpose()
    }

    async fn relation(&self, id: RelationId) -> Result<Option<Versioned<Relation>>, StoreError> {
        let sql = format!(
            "SELECT {} FROM relations WHERE id = ?",
            relation_columns("")
        );
        let row: Option<RelationRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(RelationRow::into_relation).transpose()
    }

    async fn relations(
        &self,
        filter: &RelationFilter,
        page: &Page,
    ) -> Result<Paged<Relation>, StoreError> {
        let sql = format!(
            "SELECT {} FROM relations WHERE (?1 IS NULL OR from_ref = ?1 OR to_ref = ?1) \
             AND (?2 IS NULL OR kind = ?2) AND (?3 = 1 OR status <> 'retracted') \
             AND (?4 IS NULL OR created_at < ?4 OR (created_at = ?4 AND id < ?5)) \
             ORDER BY created_at DESC, id DESC LIMIT ?6",
            relation_columns("")
        );
        let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<RelationRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(filter.subject.as_ref().map(ToString::to_string))
            .bind(filter.kind.map(RelationKind::as_str))
            .bind(i64::from(filter.include_retracted))
            .bind(after_key)
            .bind(after_id)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let (rows, next_cursor) = paginate(rows, limit, |r| {
            let (key, id) = r.key();
            Cursor { key, id }
        });
        let items = rows
            .into_iter()
            .map(|r| r.into_relation().map(|v| v.value))
            .collect::<Result<_, _>>()?;
        Ok(Paged { items, next_cursor })
    }

    async fn live_relation(
        &self,
        from: &SubjectRef,
        kind: RelationKind,
        to: &SubjectRef,
    ) -> Result<Option<Versioned<Relation>>, StoreError> {
        let sql = format!(
            "SELECT {} FROM relations WHERE from_ref = ? AND kind = ? AND to_ref = ? AND live = 1",
            relation_columns("")
        );
        let row: Option<RelationRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(from.to_string())
            .bind(kind.as_str())
            .bind(to.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(RelationRow::into_relation).transpose()
    }

    async fn all_live_relations(&self, limit: u32) -> Result<Vec<Versioned<Relation>>, StoreError> {
        let sql = format!(
            "SELECT {} FROM relations WHERE live = 1 ORDER BY created_at, id LIMIT ?",
            relation_columns("")
        );
        let rows: Vec<RelationRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(i64::from(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(RelationRow::into_relation).collect()
    }

    async fn entity(&self, id: EntityId) -> Result<Option<Versioned<Entity>>, StoreError> {
        let row: Option<EntityRow> = sqlx::query_as(
            "SELECT id, kind, name, created_by, created_at, version FROM entities WHERE id = ?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.read)
        .await
        .map_err(db)?;
        row.map(EntityRow::into_entity).transpose()
    }

    async fn entities(&self) -> Result<Vec<Versioned<Entity>>, StoreError> {
        let rows: Vec<EntityRow> = sqlx::query_as(
            "SELECT id, kind, name, created_by, created_at, version FROM entities \
             ORDER BY lower(name), id",
        )
        .fetch_all(&self.read)
        .await
        .map_err(db)?;
        rows.into_iter().map(EntityRow::into_entity).collect()
    }

    async fn entity_in_use(&self, id: EntityId) -> Result<bool, StoreError> {
        let reference = SubjectRef::Entity(id).to_string();
        let quoted = format!("\"{reference}\"");
        let fact: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM facts WHERE live = 1 \
             AND (subject_ref = ?1 OR (predicate = 'depends_on' AND value = ?2)) LIMIT 1",
        )
        .bind(&reference)
        .bind(&quoted)
        .fetch_optional(&self.read)
        .await
        .map_err(db)?;
        if fact.is_some() {
            return Ok(true);
        }
        let relation: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM relations WHERE live = 1 AND (from_ref = ?1 OR to_ref = ?1) LIMIT 1",
        )
        .bind(&reference)
        .fetch_optional(&self.read)
        .await
        .map_err(db)?;
        Ok(relation.is_some())
    }
}
