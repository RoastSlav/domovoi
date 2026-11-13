//! Signals, incidents, their timeline and incident search.
//!
//! Incidents and signals keep their full state as JSON in `body`, since their fields are
//! only reachable through serde; the other columns exist for lookups and ordering.

use async_trait::async_trait;
use domovoi_core::ports::{
    IncidentFilter, IncidentRepo, IncidentTimelineEntry, Page, Paged, SignalRepo, StoreError,
};
use domovoi_core::{
    Fingerprint, Incident, IncidentGroup, IncidentId, IncidentStatus, Signal, SignalStatus,
};
use jiff::Timestamp;
use sqlx::{Executor, Sqlite, SqliteConnection};

use crate::codec::{Cursor, fetch_limit, from_json, from_ms, json, ms, name, paginate, uint};
use crate::db::Store;
use crate::error::{db, db_parent};
use crate::fts::match_all_in;

/// Written as `"schema_version"` inside every stored incident body.
pub(crate) const INCIDENT_BODY_SCHEMA_VERSION: u64 = 1;

/// The stored `body` of `incident`, with its body schema version.
pub(crate) fn incident_body(incident: &Incident) -> Result<String, StoreError> {
    let mut body = serde_json::to_value(incident)
        .map_err(|e| StoreError::Backend(format!("cannot encode incident: {e}")))?;
    if let serde_json::Value::Object(fields) = &mut body {
        fields.insert(
            "schema_version".to_owned(),
            serde_json::json!(INCIDENT_BODY_SCHEMA_VERSION),
        );
    }
    serde_json::to_string(&body)
        .map_err(|e| StoreError::Backend(format!("cannot encode incident: {e}")))
}

/// Inserts an incident row with version 1.
pub(crate) async fn insert_incident(
    executor: impl Executor<'_, Database = Sqlite>,
    incident: &Incident,
) -> Result<(), StoreError> {
    const SQL: &str = "INSERT INTO incidents \
        (id, fingerprint, host_id, check_key, target, title, summary_text, status, severity, \
         opened_at, resolved_at, body) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
    sqlx::query(SQL)
        .bind(incident.id().to_string())
        .bind(name(&incident.fingerprint())?)
        .bind(incident.host().to_string())
        .bind(incident.check().to_string())
        .bind(incident.target().map(ToString::to_string))
        .bind(incident.title())
        .bind(summary_text(incident))
        .bind(status_text(incident.status()))
        .bind(name(&incident.severity())?)
        .bind(ms(incident.opened_at()))
        .bind(incident.resolved_at().map(ms))
        .bind(incident_body(incident)?)
        .execute(executor)
        .await
        .map_err(db)?;
    Ok(())
}

/// Replaces an incident's columns and raises its version by one.
///
/// With `expected`, the update only applies at that version. Returns the rows affected.
pub(crate) async fn update_incident(
    executor: impl Executor<'_, Database = Sqlite>,
    incident: &Incident,
    expected: Option<u64>,
) -> Result<u64, StoreError> {
    const SQL: &str = "UPDATE incidents SET fingerprint = ?, host_id = ?, check_key = ?, \
        target = ?, title = ?, summary_text = ?, status = ?, severity = ?, opened_at = ?, \
        resolved_at = ?, body = ?, version = version + 1 \
        WHERE id = ? AND (? IS NULL OR version = ?)";
    let expected = expected.map(|v| i64::try_from(v).unwrap_or(i64::MAX));
    let done = sqlx::query(SQL)
        .bind(name(&incident.fingerprint())?)
        .bind(incident.host().to_string())
        .bind(incident.check().to_string())
        .bind(incident.target().map(ToString::to_string))
        .bind(incident.title())
        .bind(summary_text(incident))
        .bind(status_text(incident.status()))
        .bind(name(&incident.severity())?)
        .bind(ms(incident.opened_at()))
        .bind(incident.resolved_at().map(ms))
        .bind(incident_body(incident)?)
        .bind(incident.id().to_string())
        .bind(expected)
        .bind(expected)
        .execute(executor)
        .await
        .map_err(db)?;
    Ok(done.rows_affected())
}

/// Stores a signal and, if given, links it to an incident.
pub(crate) async fn insert_signal_and_link(
    conn: &mut SqliteConnection,
    signal: &Signal,
    incident: Option<IncidentId>,
) -> Result<(), StoreError> {
    const SIGNAL: &str = "INSERT INTO signals \
        (id, host_id, check_key, target, status, severity, observed_at, body) \
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)";
    const LINK: &str = "INSERT INTO incident_signals (incident_id, signal_id) VALUES (?, ?)";
    let existing: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM signals WHERE id = ?")
        .bind(signal.id().to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(db)?;
    if existing.is_some() {
        return Err(StoreError::Conflict("signal id exists".to_owned()));
    }
    sqlx::query(SIGNAL)
        .bind(signal.id().to_string())
        .bind(signal.host().to_string())
        .bind(signal.check().to_string())
        .bind(signal.target().map(ToString::to_string))
        .bind(signal_status(signal.status()))
        .bind(name(&signal.severity())?)
        .bind(ms(signal.observed_at()))
        .bind(json(signal)?)
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    if let Some(incident) = incident {
        sqlx::query(LINK)
            .bind(incident.to_string())
            .bind(signal.id().to_string())
            .execute(&mut *conn)
            .await
            .map_err(db)?;
    }
    Ok(())
}

/// Appends a timeline entry and returns its `incident_events.seq`.
pub(crate) async fn insert_incident_event(
    executor: impl Executor<'_, Database = Sqlite>,
    entry: &IncidentTimelineEntry,
) -> Result<u64, StoreError> {
    const SQL: &str = "INSERT INTO incident_events (incident_id, at, kind, summary, detail) \
        VALUES (?, ?, ?, ?, ?) RETURNING seq";
    let (seq,): (i64,) = sqlx::query_as(SQL)
        .bind(entry.incident.to_string())
        .bind(ms(entry.at))
        .bind(&entry.kind)
        .bind(&entry.summary)
        .bind(json(&entry.detail)?)
        .fetch_one(executor)
        .await
        .map_err(db_parent)?;
    Ok(uint(seq))
}

/// What [`discard_pending_incident`] found.
pub(crate) enum DiscardOutcome {
    /// The incident and its timeline and links were deleted.
    Discarded,
    /// No incident has that id.
    Missing,
    /// The incident is not pending.
    NotPending,
}

/// Deletes a pending incident's signal links and the incident itself; its timeline cascades.
pub(crate) async fn discard_pending_incident(
    conn: &mut SqliteConnection,
    id: IncidentId,
) -> Result<DiscardOutcome, StoreError> {
    const STATUS: &str = "SELECT status FROM incidents WHERE id = ?";
    const LINKS: &str = "DELETE FROM incident_signals WHERE incident_id = ?";
    const DELETE: &str = "DELETE FROM incidents WHERE id = ? AND status = 'pending'";
    let status: Option<(String,)> = sqlx::query_as(STATUS)
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(db)?;
    match status {
        None => return Ok(DiscardOutcome::Missing),
        Some((status,)) if status != "pending" => return Ok(DiscardOutcome::NotPending),
        Some(_) => {}
    }
    sqlx::query(LINKS)
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    sqlx::query(DELETE)
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    Ok(DiscardOutcome::Discarded)
}

fn status_text(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Pending => "pending",
        IncidentStatus::Open => "open",
        IncidentStatus::Acked => "acked",
        IncidentStatus::Muted { .. } => "muted",
        IncidentStatus::Resolved => "resolved",
    }
}

/// A `LIKE` pattern matching anything that contains `text` literally, with `!` as the escape.
fn like_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '!') {
            pattern.push('!');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

fn signal_status(status: SignalStatus) -> &'static str {
    match status {
        SignalStatus::Firing => "firing",
        SignalStatus::Resolved => "resolved",
    }
}

/// The text indexed beside the title, for later full-text use.
fn summary_text(incident: &Incident) -> String {
    match incident.target() {
        Some(target) => format!("{} {target}", incident.check()),
        None => incident.check().to_string(),
    }
}

fn decode_all(rows: Vec<(String,)>) -> Result<Vec<Incident>, StoreError> {
    rows.into_iter()
        .map(|(body,)| from_json(&body, "incident"))
        .collect()
}

#[async_trait]
impl SignalRepo for Store {
    async fn insert(
        &self,
        signal: &Signal,
        incident: Option<IncidentId>,
    ) -> Result<(), StoreError> {
        let mut tx = self.write.begin().await.map_err(db)?;
        insert_signal_and_link(&mut tx, signal, incident).await?;
        tx.commit().await.map_err(db)
    }

    async fn list_for_incident(
        &self,
        incident: IncidentId,
        limit: u16,
    ) -> Result<Vec<Signal>, StoreError> {
        const SQL: &str = "SELECT s.body FROM signals s \
            JOIN incident_signals l ON l.signal_id = s.id \
            WHERE l.incident_id = ? ORDER BY s.observed_at DESC, s.id DESC LIMIT ?";
        let rows: Vec<(String,)> = sqlx::query_as(SQL)
            .bind(incident.to_string())
            .bind(i64::from(limit.clamp(1, Page::MAX_LIMIT)))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(body,)| from_json(&body, "signal"))
            .collect()
    }

    async fn purge_older_than(&self, before: Timestamp) -> Result<u64, StoreError> {
        const SQL: &str = "DELETE FROM signals WHERE observed_at < ?";
        let done = sqlx::query(SQL)
            .bind(ms(before))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(done.rows_affected())
    }
}

#[async_trait]
impl IncidentRepo for Store {
    async fn insert(&self, incident: &Incident) -> Result<(), StoreError> {
        insert_incident(&self.write, incident).await
    }

    async fn update(&self, incident: &Incident) -> Result<(), StoreError> {
        if update_incident(&self.write, incident, None).await? == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn get(&self, id: IncidentId) -> Result<Option<Incident>, StoreError> {
        const SQL: &str = "SELECT body FROM incidents WHERE id = ?";
        let row: Option<(String,)> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(body,)| from_json(&body, "incident")).transpose()
    }

    async fn find_active_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Incident>, StoreError> {
        const SQL: &str =
            "SELECT body FROM incidents WHERE fingerprint = ? AND status <> 'resolved'";
        let row: Option<(String,)> = sqlx::query_as(SQL)
            .bind(name(fingerprint)?)
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(body,)| from_json(&body, "incident")).transpose()
    }

    async fn find_recent_resolved_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
        since: Timestamp,
    ) -> Result<Option<Incident>, StoreError> {
        const SQL: &str = "SELECT body FROM incidents \
            WHERE fingerprint = ? AND status = 'resolved' AND resolved_at >= ? \
            ORDER BY resolved_at DESC, id DESC LIMIT 1";
        let row: Option<(String,)> = sqlx::query_as(SQL)
            .bind(name(fingerprint)?)
            .bind(ms(since))
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(body,)| from_json(&body, "incident")).transpose()
    }

    async fn list(
        &self,
        filter: &IncidentFilter,
        page: &Page,
    ) -> Result<Paged<Incident>, StoreError> {
        const SQL: &str = "SELECT body, opened_at, id FROM incidents \
            WHERE (?1 IS NULL OR host_id = ?1) \
              AND (?2 IS NULL OR (status <> 'resolved') = ?2) \
              AND (?3 IS NULL OR opened_at >= ?3) \
              AND (?4 IS NULL OR opened_at < ?4 OR (opened_at = ?4 AND id < ?5)) \
              AND (?7 IS NULL OR status = ?7) \
              AND (?8 IS NULL OR severity = ?8) \
              AND (?9 IS NULL OR ?9 = CASE \
                    WHEN status = 'open' THEN CASE json_extract(body, '$.verdict') \
                        WHEN 'MONITOR' THEN 'monitor' \
                        WHEN 'EXPECTED' THEN 'expected' \
                        ELSE 'needs_action' END \
                    WHEN status IN ('acked', 'muted') THEN 'handled' \
                    ELSE status END) \
              AND (?10 IS NULL OR title LIKE ?10 ESCAPE '!' OR check_key LIKE ?10 ESCAPE '!' \
                   OR target LIKE ?10 ESCAPE '!') \
            ORDER BY opened_at DESC, id DESC LIMIT ?6";
        let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<(String, i64, String)> = sqlx::query_as(SQL)
            .bind(filter.host.map(|h| h.to_string()))
            .bind(filter.active)
            .bind(filter.opened_since.map(ms))
            .bind(after_key)
            .bind(after_id)
            .bind(fetch_limit(limit))
            .bind(filter.status.as_deref())
            .bind(filter.severity.map(|s| name(&s)).transpose()?)
            .bind(filter.group.map(IncidentGroup::name))
            .bind(filter.text.as_deref().map(like_pattern))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let (rows, next_cursor) = paginate(rows, limit, |(_, key, id)| Cursor {
            key: *key,
            id: id.clone(),
        });
        let items = rows
            .iter()
            .map(|(body, _, _)| from_json(body, "incident"))
            .collect::<Result<_, _>>()?;
        Ok(Paged { items, next_cursor })
    }

    async fn discard_pending(&self, id: IncidentId) -> Result<(), StoreError> {
        let mut tx = self.write.begin().await.map_err(db)?;
        let outcome = discard_pending_incident(&mut tx, id).await?;
        tx.commit().await.map_err(db)?;
        match outcome {
            DiscardOutcome::Discarded => Ok(()),
            DiscardOutcome::Missing => Err(StoreError::NotFound),
            DiscardOutcome::NotPending => Err(StoreError::Conflict(
                "only pending incidents are discarded".to_owned(),
            )),
        }
    }

    async fn append_event(&self, entry: &IncidentTimelineEntry) -> Result<(), StoreError> {
        insert_incident_event(&self.write, entry).await.map(|_| ())
    }

    async fn timeline(&self, id: IncidentId) -> Result<Vec<IncidentTimelineEntry>, StoreError> {
        const SQL: &str = "SELECT at, kind, summary, detail FROM incident_events \
            WHERE incident_id = ? ORDER BY seq";
        let rows: Vec<(i64, String, String, String)> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(at, kind, summary, detail)| {
                Ok(IncidentTimelineEntry {
                    incident: id,
                    at: from_ms(at)?,
                    kind,
                    summary,
                    detail: from_json(&detail, "timeline detail")?,
                })
            })
            .collect()
    }

    async fn search(&self, text: &str, limit: u16) -> Result<Vec<Incident>, StoreError> {
        const SQL: &str = "SELECT i.body FROM incidents_fts \
            JOIN incidents i ON i.rowid = incidents_fts.rowid \
            WHERE incidents_fts MATCH ? ORDER BY i.opened_at DESC, i.id DESC LIMIT ?";
        let Some(query) = match_all_in("title", text) else {
            return Ok(Vec::new());
        };
        let rows: Vec<(String,)> = sqlx::query_as(SQL)
            .bind(query)
            .bind(i64::from(limit.clamp(1, Page::MAX_LIMIT)))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        decode_all(rows)
    }
}
