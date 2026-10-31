//! Baselines, ack rules, maintenance windows, advisories and digest facts.

use async_trait::async_trait;
use domovoi_core::ports::{AckRepo, AdvisoryRepo, BaselineRepo, FactRepo, StoreError};
use domovoi_core::{
    AckRule, AckRuleId, Advisory, AdvisoryId, BaselineEntry, BaselineKind, DigestFact, HostId,
    MaintenanceWindow,
};
use jiff::Timestamp;

use crate::codec::{from_json, from_ms, from_ms_opt, from_name, json, ms, name, parse};
use crate::db::Store;
use crate::error::db;

fn not_found_if_none(done: sqlx::sqlite::SqliteQueryResult) -> Result<(), StoreError> {
    if done.rows_affected() == 0 {
        Err(StoreError::NotFound)
    } else {
        Ok(())
    }
}

#[async_trait]
impl BaselineRepo for Store {
    async fn upsert(&self, entry: &BaselineEntry) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO baselines \
            (host_id, kind, key, criticality, learned_at, last_seen_at, managed) \
            VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (host_id, kind, key) DO UPDATE SET \
            criticality = excluded.criticality, learned_at = excluded.learned_at, \
            last_seen_at = excluded.last_seen_at, managed = excluded.managed";
        sqlx::query(SQL)
            .bind(entry.host.to_string())
            .bind(name(&entry.kind)?)
            .bind(&entry.key)
            .bind(name(&entry.criticality)?)
            .bind(ms(entry.learned_at))
            .bind(ms(entry.last_seen_at))
            .bind(entry.managed)
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn list(&self, host: HostId) -> Result<Vec<BaselineEntry>, StoreError> {
        // Ties on key follow the kind's declaration order, as the domain type does.
        const SQL: &str = "SELECT kind, key, criticality, learned_at, last_seen_at, managed \
            FROM baselines WHERE host_id = ? ORDER BY key, \
            CASE kind WHEN 'container' THEN 0 WHEN 'unit' THEN 1 ELSE 2 END";
        let rows: Vec<(String, String, String, i64, i64, bool)> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(kind, key, criticality, learned, seen, managed)| {
                Ok(BaselineEntry {
                    host,
                    kind: from_name(&kind, "baseline kind")?,
                    key,
                    criticality: from_name(&criticality, "criticality")?,
                    learned_at: from_ms(learned)?,
                    last_seen_at: from_ms(seen)?,
                    managed,
                })
            })
            .collect()
    }

    async fn delete(&self, host: HostId, kind: BaselineKind, key: &str) -> Result<(), StoreError> {
        const SQL: &str = "DELETE FROM baselines WHERE host_id = ? AND kind = ? AND key = ?";
        let done = sqlx::query(SQL)
            .bind(host.to_string())
            .bind(name(&kind)?)
            .bind(key)
            .execute(&self.write)
            .await
            .map_err(db)?;
        not_found_if_none(done)
    }
}

#[async_trait]
impl AckRepo for Store {
    async fn put_rule(&self, rule: &AckRule) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO ack_rules \
            (id, host_id, check_key, target_pattern, predicates, note, expires_at) \
            VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET \
            host_id = excluded.host_id, check_key = excluded.check_key, \
            target_pattern = excluded.target_pattern, predicates = excluded.predicates, \
            note = excluded.note, expires_at = excluded.expires_at";
        sqlx::query(SQL)
            .bind(rule.id.to_string())
            .bind(rule.host.map(|h| h.to_string()))
            .bind(rule.check.to_string())
            .bind(rule.target_pattern.as_ref().map(name).transpose()?)
            .bind(json(&rule.predicates)?)
            .bind(&rule.note)
            .bind(rule.expires_at.map(ms))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn delete_rule(&self, id: AckRuleId) -> Result<(), StoreError> {
        let done = sqlx::query("DELETE FROM ack_rules WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        not_found_if_none(done)
    }

    async fn list_rules(&self) -> Result<Vec<AckRule>, StoreError> {
        const SQL: &str = "SELECT id, host_id, check_key, target_pattern, predicates, note, \
            expires_at FROM ack_rules ORDER BY id";
        type Row = (
            String,
            Option<String>,
            String,
            Option<String>,
            String,
            String,
            Option<i64>,
        );
        let rows: Vec<Row> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(id, host, check, pattern, predicates, note, expires)| {
                Ok(AckRule {
                    id: parse(&id, "ack rule id")?,
                    host: host.as_deref().map(|h| parse(h, "host id")).transpose()?,
                    check: from_name(&check, "check key")?,
                    target_pattern: pattern
                        .as_deref()
                        .map(|p| from_name(p, "target pattern"))
                        .transpose()?,
                    predicates: from_json(&predicates, "ack predicates")?,
                    note,
                    expires_at: from_ms_opt(expires)?,
                })
            })
            .collect()
    }

    async fn put_window(&self, window: &MaintenanceWindow) -> Result<(), StoreError> {
        const SQL: &str = "INSERT OR IGNORE INTO maintenance_windows \
            (host_id, starts_at, ends_at, note) VALUES (?, ?, ?, ?)";
        sqlx::query(SQL)
            .bind(window.host.to_string())
            .bind(ms(window.starts_at))
            .bind(ms(window.ends_at))
            .bind(&window.note)
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn windows_for(&self, host: HostId) -> Result<Vec<MaintenanceWindow>, StoreError> {
        const SQL: &str = "SELECT starts_at, ends_at, note FROM maintenance_windows \
            WHERE host_id = ? ORDER BY starts_at, rowid";
        let rows: Vec<(i64, i64, String)> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(starts, ends, note)| {
                Ok(MaintenanceWindow {
                    host,
                    starts_at: from_ms(starts)?,
                    ends_at: from_ms(ends)?,
                    note,
                })
            })
            .collect()
    }

    async fn purge_windows_ended_before(&self, before: Timestamp) -> Result<u64, StoreError> {
        let done = sqlx::query("DELETE FROM maintenance_windows WHERE ends_at <= ?")
            .bind(ms(before))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(done.rows_affected())
    }
}

#[async_trait]
impl AdvisoryRepo for Store {
    async fn upsert(&self, advisory: &Advisory) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO advisories \
            (id, kind, host_id, title, detail, created_at, resolved_at) \
            VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, \
            host_id = excluded.host_id, title = excluded.title, detail = excluded.detail, \
            created_at = excluded.created_at, resolved_at = excluded.resolved_at";
        sqlx::query(SQL)
            .bind(advisory.id.to_string())
            .bind(name(&advisory.kind)?)
            .bind(advisory.host.map(|h| h.to_string()))
            .bind(&advisory.title)
            .bind(&advisory.detail)
            .bind(ms(advisory.created_at))
            .bind(advisory.resolved_at.map(ms))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn list_open(&self) -> Result<Vec<Advisory>, StoreError> {
        const SQL: &str = "SELECT id, kind, host_id, title, detail, created_at FROM advisories \
            WHERE resolved_at IS NULL ORDER BY created_at DESC, id DESC";
        let rows: Vec<(String, String, Option<String>, String, String, i64)> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(id, kind, host, title, detail, created)| {
                Ok(Advisory {
                    id: parse(&id, "advisory id")?,
                    kind: from_name(&kind, "advisory kind")?,
                    host: host.as_deref().map(|h| parse(h, "host id")).transpose()?,
                    title,
                    detail,
                    created_at: from_ms(created)?,
                    resolved_at: None,
                })
            })
            .collect()
    }

    async fn resolve(&self, id: AdvisoryId, at: Timestamp) -> Result<(), StoreError> {
        const SQL: &str =
            "UPDATE advisories SET resolved_at = coalesce(resolved_at, ?) WHERE id = ?";
        let done = sqlx::query(SQL)
            .bind(ms(at))
            .bind(id.to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        not_found_if_none(done)
    }
}

#[async_trait]
impl FactRepo for Store {
    async fn record(&self, fact: &DigestFact) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO digest_facts \
            (host_id, key, label, value, unit, observed_at) VALUES (?, ?, ?, ?, ?, ?)";
        sqlx::query(SQL)
            .bind(fact.host.to_string())
            .bind(&fact.key)
            .bind(&fact.label)
            .bind(json(&fact.value)?)
            .bind(fact.unit.as_deref())
            .bind(ms(fact.observed_at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn list(
        &self,
        host: HostId,
        from: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<DigestFact>, StoreError> {
        const SQL: &str = "SELECT key, label, value, unit, observed_at FROM digest_facts \
            WHERE host_id = ? AND observed_at >= ? AND observed_at < ? ORDER BY observed_at, id";
        let rows: Vec<(String, String, String, Option<String>, i64)> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .bind(ms(from))
            .bind(ms(until))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(key, label, value, unit, observed)| {
                Ok(DigestFact {
                    host,
                    key,
                    label,
                    value: from_json(&value, "fact value")?,
                    unit,
                    observed_at: from_ms(observed)?,
                })
            })
            .collect()
    }
}
