//! Hosts, agents and enrollment tokens.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::ports::{EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{
    AgentId, AgentIdentity, AgentReport, AuthenticatedReport, Host, HostId, HostName,
    REPORT_REFRESH, ReportOutcome,
};
use jiff::Timestamp;

use crate::codec::{from_json, from_ms, from_ms_opt, from_name, hash32, json, ms, name, parse};
use crate::db::Store;
use crate::error::{corrupt, db};

#[derive(sqlx::FromRow)]
struct HostRow {
    id: String,
    name: String,
    kind: String,
    labels: String,
    created_at: i64,
}

impl HostRow {
    fn into_host(self) -> Result<Host, StoreError> {
        Ok(Host {
            id: parse(&self.id, "host id")?,
            name: from_name(&self.name, "host name")?,
            kind: from_name(&self.kind, "host kind")?,
            labels: from_json::<BTreeMap<String, String>>(&self.labels, "host labels")?,
            created_at: from_ms(self.created_at)?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct AgentRow {
    agent_id: String,
    host_id: String,
    public_key: Vec<u8>,
    version: String,
    enrolled_at: i64,
    revoked_at: Option<i64>,
}

impl AgentRow {
    fn into_agent(self) -> Result<AgentIdentity, StoreError> {
        Ok(AgentIdentity {
            agent: parse(&self.agent_id, "agent id")?,
            host: parse(&self.host_id, "host id")?,
            public_key: hash32(&self.public_key, "agent public key")?,
            version: self.version,
            enrolled_at: from_ms(self.enrolled_at)?,
            revoked_at: from_ms_opt(self.revoked_at)?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ReportRow {
    reported_protocol: Option<String>,
    reported_release: Option<String>,
    last_authenticated_at: Option<i64>,
    incompatible_since: Option<i64>,
}

impl ReportRow {
    /// The stored report, or `None` when the columns are NULL (an agent that never
    /// authenticated since the columns existed).
    fn into_report(self) -> Result<Option<AgentReport>, StoreError> {
        let (Some(protocol), Some(release), Some(last)) = (
            self.reported_protocol,
            self.reported_release,
            self.last_authenticated_at,
        ) else {
            return Ok(None);
        };
        Ok(Some(AgentReport {
            protocol: protocol
                .parse()
                .map_err(|e| corrupt("reported protocol", e))?,
            release,
            last_authenticated_at: from_ms(last)?,
            incompatible_since: from_ms_opt(self.incompatible_since)?,
        }))
    }
}

macro_rules! host_columns {
    () => {
        "SELECT id, name, kind, labels, created_at FROM hosts"
    };
}

async fn host_exists(conn: &mut sqlx::SqliteConnection, host: HostId) -> Result<bool, StoreError> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM hosts WHERE id = ?")
        .bind(host.to_string())
        .fetch_optional(conn)
        .await
        .map_err(db)?;
    Ok(row.is_some())
}

#[async_trait]
impl HostRepo for Store {
    async fn upsert_host(&self, host: &Host) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO hosts (id, name, kind, labels, created_at) \
            VALUES (?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET name = excluded.name, \
            kind = excluded.kind, labels = excluded.labels, created_at = excluded.created_at";
        sqlx::query(SQL)
            .bind(host.id.to_string())
            .bind(host.name.as_str())
            .bind(name(&host.kind)?)
            .bind(json(&host.labels)?)
            .bind(ms(host.created_at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn get_host(&self, id: HostId) -> Result<Option<Host>, StoreError> {
        const SQL: &str = concat!(host_columns!(), " WHERE id = ?");
        let row: Option<HostRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(HostRow::into_host).transpose()
    }

    async fn get_host_by_name(&self, host: &HostName) -> Result<Option<Host>, StoreError> {
        const SQL: &str = concat!(host_columns!(), " WHERE name = ?");
        let row: Option<HostRow> = sqlx::query_as(SQL)
            .bind(host.as_str())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(HostRow::into_host).transpose()
    }

    async fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        const SQL: &str = concat!(host_columns!(), " ORDER BY name");
        let rows: Vec<HostRow> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(HostRow::into_host).collect()
    }

    async fn put_agent(&self, agent: &AgentIdentity) -> Result<(), StoreError> {
        const REVOKED: &str = "SELECT revoked_at FROM agents WHERE agent_id = ?";
        const SQL: &str = "INSERT INTO agents \
            (agent_id, host_id, public_key, version, enrolled_at, revoked_at) \
            VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (agent_id) DO UPDATE SET \
            host_id = excluded.host_id, public_key = excluded.public_key, \
            version = excluded.version, enrolled_at = excluded.enrolled_at, \
            revoked_at = excluded.revoked_at";
        let mut tx = self.write.begin().await.map_err(db)?;
        if !host_exists(&mut tx, agent.host).await? {
            return Err(StoreError::NotFound);
        }
        let existing: Option<(Option<i64>,)> = sqlx::query_as(REVOKED)
            .bind(agent.agent.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
        if matches!(existing, Some((Some(_),))) {
            return Err(StoreError::Conflict(
                "a revoked agent never comes back".to_owned(),
            ));
        }
        sqlx::query(SQL)
            .bind(agent.agent.to_string())
            .bind(agent.host.to_string())
            .bind(&agent.public_key[..])
            .bind(&agent.version)
            .bind(ms(agent.enrolled_at))
            .bind(agent.revoked_at.map(ms))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)
    }

    async fn get_agent(&self, id: AgentId) -> Result<Option<AgentIdentity>, StoreError> {
        const SQL: &str = "SELECT agent_id, host_id, public_key, version, enrolled_at, \
            revoked_at FROM agents WHERE agent_id = ?";
        let row: Option<AgentRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(AgentRow::into_agent).transpose()
    }

    async fn live_agent(&self, host: HostId) -> Result<Option<AgentIdentity>, StoreError> {
        const SQL: &str = "SELECT agent_id, host_id, public_key, version, enrolled_at,             revoked_at FROM agents WHERE host_id = ? AND revoked_at IS NULL";
        let row: Option<AgentRow> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(AgentRow::into_agent).transpose()
    }

    async fn revoke_agent(&self, id: AgentId, at: Timestamp) -> Result<(), StoreError> {
        const SQL: &str =
            "UPDATE agents SET revoked_at = coalesce(revoked_at, ?) WHERE agent_id = ?";
        let done = sqlx::query(SQL)
            .bind(ms(at))
            .bind(id.to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn agent_report(&self, agent: AgentId) -> Result<Option<AgentReport>, StoreError> {
        const SQL: &str = "SELECT reported_protocol, reported_release, last_authenticated_at, \
            incompatible_since FROM agents WHERE agent_id = ?";
        let row: Option<ReportRow> = sqlx::query_as(SQL)
            .bind(agent.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(ReportRow::into_report)
            .transpose()
            .map(Option::flatten)
    }

    async fn record_agent_report(
        &self,
        agent: AgentId,
        seen: &AuthenticatedReport,
        now: Timestamp,
    ) -> Result<ReportOutcome, StoreError> {
        const READ: &str = "SELECT reported_protocol, reported_release, last_authenticated_at, \
            incompatible_since FROM agents WHERE agent_id = ?";
        const WRITE_ALL: &str = "UPDATE agents SET reported_protocol = ?, reported_release = ?, \
            last_authenticated_at = ?, incompatible_since = ? WHERE agent_id = ?";
        const REFRESH: &str = "UPDATE agents SET last_authenticated_at = ? WHERE agent_id = ?";
        let mut tx = self.write.begin().await.map_err(db)?;
        let row: Option<ReportRow> = sqlx::query_as(READ)
            .bind(agent.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
        let Some(stored) = row else {
            return Err(StoreError::NotFound);
        };
        let existing_since = from_ms_opt(stored.incompatible_since)?;
        let stored_last = stored.last_authenticated_at.map(from_ms).transpose()?;
        let protocol = seen.protocol.to_string();
        let changed = stored.reported_protocol.as_deref() != Some(protocol.as_str())
            || stored.reported_release.as_deref() != Some(seen.release.as_str())
            || existing_since.is_some() != !seen.compatible;
        let previous = ReportRow {
            reported_protocol: stored.reported_protocol,
            reported_release: stored.reported_release,
            last_authenticated_at: stored.last_authenticated_at,
            incompatible_since: stored.incompatible_since,
        }
        .into_report()?;
        let since = if seen.compatible {
            None
        } else {
            existing_since.or(Some(now))
        };
        if changed {
            sqlx::query(WRITE_ALL)
                .bind(&protocol)
                .bind(&seen.release)
                .bind(ms(now))
                .bind(since.map(ms))
                .bind(agent.to_string())
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        } else if stored_last.is_none_or(|last| now.duration_since(last) >= REPORT_REFRESH) {
            sqlx::query(REFRESH)
                .bind(ms(now))
                .bind(agent.to_string())
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        Ok(ReportOutcome { changed, previous })
    }

    async fn create_enrollment_token(&self, token: &EnrollmentToken) -> Result<(), StoreError> {
        const SQL: &str =
            "INSERT INTO enrollment_tokens (token_hash, host_id, expires_at) VALUES (?, ?, ?)";
        let mut tx = self.write.begin().await.map_err(db)?;
        if !host_exists(&mut tx, token.host).await? {
            return Err(StoreError::NotFound);
        }
        sqlx::query(SQL)
            .bind(&token.token_hash[..])
            .bind(token.host.to_string())
            .bind(ms(token.expires_at))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)
    }

    async fn consume_enrollment_token(
        &self,
        token_hash: &[u8; 32],
        now: Timestamp,
    ) -> Result<HostId, StoreError> {
        // One statement, so two concurrent consumers can never both succeed.
        const SQL: &str = "UPDATE enrollment_tokens SET used_at = ?1 \
            WHERE token_hash = ?2 AND used_at IS NULL AND expires_at > ?1 RETURNING host_id";
        let row: Option<(String,)> = sqlx::query_as(SQL)
            .bind(ms(now))
            .bind(&token_hash[..])
            .fetch_optional(&self.write)
            .await
            .map_err(db)?;
        match row {
            Some((host,)) => parse(&host, "host id"),
            None => Err(StoreError::NotFound),
        }
    }
}
