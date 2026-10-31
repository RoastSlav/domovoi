//! Signals, incidents and incident timelines.

use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;

use super::{Page, Paged, StoreError};
use crate::fingerprint::Fingerprint;
use crate::id::{HostId, IncidentId};
use crate::incident::Incident;
use crate::signal::Signal;

/// Stored signals.
#[async_trait]
pub trait SignalRepo: Send + Sync {
    /// Stores a signal, linked to the incident it was folded into, if any.
    /// [`StoreError::Conflict`] when the signal id is already stored.
    async fn insert(&self, signal: &Signal, incident: Option<IncidentId>)
    -> Result<(), StoreError>;

    /// Up to `limit` signals linked to `incident`, newest `observed_at` first.
    async fn list_for_incident(
        &self,
        incident: IncidentId,
        limit: u16,
    ) -> Result<Vec<Signal>, StoreError>;

    /// Deletes every signal observed before `before` and returns how many were deleted.
    async fn purge_older_than(&self, before: Timestamp) -> Result<u64, StoreError>;
}

/// Which incidents a listing returns. The default returns all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncidentFilter {
    /// Only incidents on this host.
    pub host: Option<HostId>,
    /// `Some(true)`: only active (pending, open, acked, muted); `Some(false)`: only resolved.
    pub active: Option<bool>,
    /// Only incidents opened at or after this instant.
    pub opened_since: Option<Timestamp>,
}

/// One line in an incident's timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct IncidentTimelineEntry {
    /// The incident.
    pub incident: IncidentId,
    /// When it happened.
    pub at: Timestamp,
    /// Dotted kind, for example `status.changed` or `triage.report`.
    pub kind: String,
    /// One line for humans.
    pub summary: String,
    /// Structured detail; `Value::Null` when there is none.
    pub detail: Value,
}

/// Stored incidents. Only the incident manager writes (ADR 0011).
///
/// At most one *active* incident (pending, open, acked or muted) exists per fingerprint:
/// an insert or update that would create a second one returns [`StoreError::Conflict`].
/// Listings order by `opened_at`, newest first.
#[async_trait]
pub trait IncidentRepo: Send + Sync {
    /// Stores a new incident.
    async fn insert(&self, incident: &Incident) -> Result<(), StoreError>;

    /// Replaces the stored incident with the same id.
    async fn update(&self, incident: &Incident) -> Result<(), StoreError>;

    /// The incident with `id`.
    async fn get(&self, id: IncidentId) -> Result<Option<Incident>, StoreError>;

    /// The active incident with `fingerprint`, if any.
    async fn find_active_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Incident>, StoreError>;

    /// The incident with `fingerprint` resolved most recently, if it resolved at or after
    /// `since`. Used for reopening inside the reopen window.
    async fn find_recent_resolved_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
        since: Timestamp,
    ) -> Result<Option<Incident>, StoreError>;

    /// One page of incidents matching `filter`.
    async fn list(
        &self,
        filter: &IncidentFilter,
        page: &Page,
    ) -> Result<Paged<Incident>, StoreError>;

    /// Deletes a pending incident that resolved before it was confirmed, with its timeline
    /// and its signal links (the signals stay). [`StoreError::NotFound`] when it does not
    /// exist, [`StoreError::Conflict`] when it is not pending: confirmed incidents are kept.
    async fn discard_pending(&self, id: IncidentId) -> Result<(), StoreError>;

    /// Appends a timeline entry. [`StoreError::NotFound`] when the incident does not exist.
    async fn append_event(&self, entry: &IncidentTimelineEntry) -> Result<(), StoreError>;

    /// The incident's timeline in the order entries were appended. Empty for an unknown id.
    async fn timeline(&self, id: IncidentId) -> Result<Vec<IncidentTimelineEntry>, StoreError>;

    /// Up to `limit` incidents, newest first, whose title contains every word of `text` as
    /// a whole word, ignoring case. Words are runs of letters and digits; a query with no
    /// words matches nothing.
    async fn search(&self, text: &str, limit: u16) -> Result<Vec<Incident>, StoreError>;
}
