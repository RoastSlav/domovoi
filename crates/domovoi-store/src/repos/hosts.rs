//! Hosts, agents and enrollment tokens.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::ports::{EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{AgentId, AgentIdentity, Host, HostId, HostName};
use jiff::Timestamp;

use crate::codec::{from_json, from_ms, from_ms_opt, from_name, hash32, json, ms, name, parse};
use crate::db::Store;
use crate::error::db;

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
