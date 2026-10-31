//! Notification state per incident, kept as JSON with a held flag for the quiet-hours queue.

use async_trait::async_trait;
use domovoi_core::ports::{NotifyStateRepo, StoreError};
use domovoi_core::{IncidentId, NotifyState};
use jiff::Timestamp;

use crate::codec::{from_json, json, ms, parse};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl NotifyStateRepo for Store {
    async fn get(&self, incident: IncidentId) -> Result<Option<NotifyState>, StoreError> {
        const SQL: &str = "SELECT state FROM notify_state WHERE incident_id = ?";
        let row: Option<(String,)> = sqlx::query_as(SQL)
            .bind(incident.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(state,)| from_json(&state, "notify state"))
            .transpose()
    }

    async fn put(
        &self,
        incident: IncidentId,
        state: &NotifyState,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO notify_state (incident_id, held, state, updated_at) \
            VALUES (?, ?, ?, ?) ON CONFLICT (incident_id) DO UPDATE SET held = excluded.held, \
            state = excluded.state, updated_at = excluded.updated_at";
        sqlx::query(SQL)
            .bind(incident.to_string())
            .bind(state.is_held())
            .bind(json(state)?)
            .bind(ms(at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn list_held(&self) -> Result<Vec<(IncidentId, NotifyState)>, StoreError> {
        const SQL: &str = "SELECT incident_id, state FROM notify_state WHERE held = 1 \
            ORDER BY updated_at DESC, incident_id DESC";
        let rows: Vec<(String, String)> = sqlx::query_as(SQL)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(id, state)| {
                Ok((
                    parse(&id, "incident id")?,
                    from_json(&state, "notify state")?,
                ))
            })
            .collect()
    }

    async fn delete(&self, incident: IncidentId) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM notify_state WHERE incident_id = ?")
            .bind(incident.to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }
}
