//! Versioned, journal and effect reads.

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateKind, AggregateRef, ClaimToken, EffectRead, EffectRecord, EffectStatus, JournalEntry,
    JournalRead, Page, StoreError, Versioned, VersionedRead,
};
use domovoi_core::{HostId, Incident, IncidentId, Note, NoteId};

use crate::codec::{from_json, from_ms, from_ms_opt, parse, uint};
use crate::db::Store;
use crate::error::{corrupt, db};

/// The aggregate type and id of a stored journal or effect row.
fn aggregate(kind: &str, id: &str) -> Result<AggregateRef, StoreError> {
    let kind = AggregateKind::parse(kind)
        .ok_or_else(|| corrupt("aggregate type", format!("unknown kind {kind}")))?;
    Ok(AggregateRef {
        kind,
        id: id.to_owned(),
    })
}

#[derive(sqlx::FromRow)]
struct JournalRow {
    seq: i64,
    aggregate_type: String,
    aggregate_id: String,
    kind: String,
    payload_v: i64,
    payload: String,
    actor: String,
    occurred_at: i64,
}

impl JournalRow {
    fn into_entry(self) -> Result<JournalEntry, StoreError> {
        let payload_v = u32::try_from(uint(self.payload_v))
            .map_err(|_| corrupt("journal payload_v", "too large"))?;
        Ok(JournalEntry {
            seq: uint(self.seq),
            aggregate: aggregate(&self.aggregate_type, &self.aggregate_id)?,
            kind: self.kind,
            payload_v,
            payload: from_json(&self.payload, "journal payload")?,
            actor: self.actor,
            occurred_at: from_ms(self.occurred_at)?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct EffectRow {
    id: String,
    kind: String,
    aggregate_type: String,
    aggregate_id: String,
    payload: String,
    status: String,
    not_before: i64,
    attempt: i64,
    max_attempts: i64,
    dedup_key: Option<String>,
    claim_token: Option<String>,
    claim_generation: i64,
    lease_until: Option<i64>,
    runner_id: Option<String>,
    last_error: Option<String>,
    created_at: i64,
    completed_at: Option<i64>,
}

impl EffectRow {
    fn into_record(self) -> Result<EffectRecord, StoreError> {
        let attempt = u32::try_from(uint(self.attempt))
            .map_err(|_| corrupt("effect attempt", "too large"))?;
        let max_attempts = u32::try_from(uint(self.max_attempts))
            .map_err(|_| corrupt("effect max_attempts", "too large"))?;
        Ok(EffectRecord {
            id: parse(&self.id, "effect id")?,
            kind: self.kind.clone(),
            aggregate: aggregate(&self.aggregate_type, &self.aggregate_id)?,
            payload: from_json(&self.payload, "effect payload")?,
            status: self.status,
            not_before: from_ms(self.not_before)?,
            attempt,
            max_attempts,
            dedup_key: self.dedup_key,
            claim_token: self
                .claim_token
                .as_deref()
                .map(|text| parse::<ClaimToken>(text, "claim token"))
                .transpose()?,
            claim_generation: uint(self.claim_generation),
            lease_until: from_ms_opt(self.lease_until)?,
            runner_id: self.runner_id,
            last_error: self.last_error,
            created_at: from_ms(self.created_at)?,
            completed_at: from_ms_opt(self.completed_at)?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct NoteVersionRow {
    id: String,
    host_id: Option<String>,
    title: String,
    body: String,
    tags: String,
    updated_at: i64,
    version: i64,
}

impl NoteVersionRow {
    fn into_versioned(self) -> Result<Versioned<Note>, StoreError> {
        Ok(Versioned {
            value: Note {
                id: parse(&self.id, "note id")?,
                host: self
                    .host_id
                    .as_deref()
                    .map(|host| parse::<HostId>(host, "host id"))
                    .transpose()?,
                title: self.title,
                body_markdown: self.body,
                tags: from_json(&self.tags, "note tags")?,
                updated_at: from_ms(self.updated_at)?,
            },
            version: uint(self.version),
        })
    }
}

fn clamp(limit: u16) -> i64 {
    i64::from(limit.clamp(1, Page::MAX_LIMIT))
}

#[async_trait]
impl VersionedRead for Store {
    async fn incident(&self, id: IncidentId) -> Result<Option<Versioned<Incident>>, StoreError> {
        const SQL: &str = "SELECT body, version FROM incidents WHERE id = ?";
        let row: Option<(String, i64)> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(body, version)| {
            Ok(Versioned {
                value: from_json(&body, "incident")?,
                version: uint(version),
            })
        })
        .transpose()
    }

    async fn active_incidents(&self) -> Result<Vec<Versioned<Incident>>, StoreError> {
        const SQL: &str = "SELECT body, version FROM incidents \
            WHERE status <> 'resolved' ORDER BY opened_at DESC, id DESC";
        let rows: Vec<(String, i64)> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(body, version)| {
                Ok(Versioned {
                    value: from_json(&body, "incident")?,
                    version: uint(version),
                })
            })
            .collect()
    }

    async fn note(&self, id: NoteId) -> Result<Option<Versioned<Note>>, StoreError> {
        const SQL: &str = "SELECT id, host_id, title, body, tags, updated_at, version \
            FROM notes WHERE id = ?";
        let row: Option<NoteVersionRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(NoteVersionRow::into_versioned).transpose()
    }
}

#[async_trait]
impl JournalRead for Store {
    async fn entries_after(
        &self,
        after_seq: u64,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        const SQL: &str = "SELECT seq, aggregate_type, aggregate_id, kind, payload_v, payload, \
            actor, occurred_at FROM journal WHERE seq > ? ORDER BY seq LIMIT ?";
        let rows: Vec<JournalRow> = sqlx::query_as(SQL)
            .bind(i64::try_from(after_seq).unwrap_or(i64::MAX))
            .bind(clamp(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(JournalRow::into_entry).collect()
    }

    async fn entries_for(
        &self,
        aggregate_ref: &AggregateRef,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        const SQL: &str = "SELECT seq, aggregate_type, aggregate_id, kind, payload_v, payload, \
            actor, occurred_at FROM journal \
            WHERE aggregate_type = ? AND aggregate_id = ? ORDER BY seq LIMIT ?";
        let rows: Vec<JournalRow> = sqlx::query_as(SQL)
            .bind(aggregate_ref.kind.as_str())
            .bind(&aggregate_ref.id)
            .bind(clamp(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(JournalRow::into_entry).collect()
    }

    async fn head_seq(&self) -> Result<u64, StoreError> {
        let seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM journal")
            .fetch_one(&self.read)
            .await
            .map_err(db)?;
        Ok(uint(seq))
    }
}

#[async_trait]
impl EffectRead for Store {
    async fn effect(&self, id: domovoi_core::EffectId) -> Result<Option<EffectRecord>, StoreError> {
        const SQL: &str = "SELECT id, kind, aggregate_type, aggregate_id, payload, status, \
            not_before, attempt, max_attempts, dedup_key, claim_token, claim_generation, \
            lease_until, runner_id, last_error, created_at, completed_at \
            FROM effects WHERE id = ?";
        let row: Option<EffectRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(EffectRow::into_record).transpose()
    }

    async fn effects_with_status(
        &self,
        status: EffectStatus,
        limit: u16,
    ) -> Result<Vec<EffectRecord>, StoreError> {
        const SQL: &str = "SELECT id, kind, aggregate_type, aggregate_id, payload, status, \
            not_before, attempt, max_attempts, dedup_key, claim_token, claim_generation, \
            lease_until, runner_id, last_error, created_at, completed_at \
            FROM effects WHERE status = ? ORDER BY seq LIMIT ?";
        let rows: Vec<EffectRow> = sqlx::query_as(SQL)
            .bind(status.as_str())
            .bind(clamp(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(EffectRow::into_record).collect()
    }
}
