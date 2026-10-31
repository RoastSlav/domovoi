//! Notification state per incident.

use async_trait::async_trait;
use jiff::Timestamp;

use super::StoreError;
use crate::id::IncidentId;
use crate::notify::NotifyState;

/// What the notification policy remembers about each incident: card and thread references,
/// reminders, folded replies and held alerts.
#[async_trait]
pub trait NotifyStateRepo: Send + Sync {
    /// The state of `incident`, or `None` if nothing was said about it yet.
    async fn get(&self, incident: IncidentId) -> Result<Option<NotifyState>, StoreError>;

    /// Stores the state of `incident`, replacing any earlier one.
    async fn put(
        &self,
        incident: IncidentId,
        state: &NotifyState,
        at: Timestamp,
    ) -> Result<(), StoreError>;

    /// Every incident with an alert held for the end of quiet hours, most recently
    /// updated first.
    async fn list_held(&self) -> Result<Vec<(IncidentId, NotifyState)>, StoreError>;

    /// Forgets the state of `incident`. Forgetting an unknown incident succeeds.
    async fn delete(&self, incident: IncidentId) -> Result<(), StoreError>;
}
