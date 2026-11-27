//! Notes, baselines, ack rules, maintenance windows, advisories and digest facts.

use async_trait::async_trait;
use jiff::Timestamp;

use super::StoreError;
use crate::ack::{AckRule, MaintenanceWindow};
use crate::advisory::Advisory;
use crate::baseline::{BaselineEntry, BaselineKind};
use crate::digest::DigestFact;
use crate::id::{AckRuleId, AdvisoryId, HostId, NoteId};
use crate::note::Note;

/// One note search result.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteHit {
    /// The matching note.
    pub note: Note,
    /// A short non-empty excerpt around the match.
    pub snippet: String,
    /// Relevance; higher is better. Only comparable within one search.
    pub score: f64,
}

/// Runbook notes.
#[async_trait]
pub trait NoteRepo: Send + Sync {
    /// Inserts a note or replaces the one with the same id.
    async fn upsert(&self, note: &Note) -> Result<(), StoreError>;

    /// The note with `id`.
    async fn get(&self, id: NoteId) -> Result<Option<Note>, StoreError>;

    /// Deletes a note.
    async fn delete(&self, id: NoteId) -> Result<(), StoreError>;

    /// Notes whose `host` equals `host` exactly (`None` lists global notes), most recently
    /// updated first.
    async fn list(&self, host: Option<HostId>) -> Result<Vec<Note>, StoreError>;

    /// Up to `limit` notes whose title, body or tags contain every word of `query` as a
    /// whole word, ignoring case, best score first. With `host = Some(h)` only `h`'s notes
    /// and global notes are searched; with `None`, every note. A query with no words
    /// matches nothing.
    async fn search(
        &self,
        query: &str,
        host: Option<HostId>,
        limit: u16,
    ) -> Result<Vec<NoteHit>, StoreError>;
}

/// Learned baselines. An entry is identified by host, kind and key.
#[async_trait]
pub trait BaselineRepo: Send + Sync {
    /// Inserts an entry or replaces the one with the same host, kind and key.
    async fn upsert(&self, entry: &BaselineEntry) -> Result<(), StoreError>;

    /// Every entry for `host`, sorted by key.
    async fn list(&self, host: HostId) -> Result<Vec<BaselineEntry>, StoreError>;

    /// Deletes one entry.
    async fn delete(&self, host: HostId, kind: BaselineKind, key: &str) -> Result<(), StoreError>;
}

/// Ack rules and maintenance windows.
#[async_trait]
pub trait AckRepo: Send + Sync {
    /// Inserts a rule or replaces the one with the same id.
    async fn put_rule(&self, rule: &AckRule) -> Result<(), StoreError>;

    /// Deletes a rule.
    async fn delete_rule(&self, id: AckRuleId) -> Result<(), StoreError>;

    /// Every rule, expired or not, sorted by id.
    async fn list_rules(&self) -> Result<Vec<AckRule>, StoreError>;

    /// Stores a window. Storing an identical window again keeps one copy.
    async fn put_window(&self, window: &MaintenanceWindow) -> Result<(), StoreError>;

    /// Every stored window for `host`, sorted by start.
    async fn windows_for(&self, host: HostId) -> Result<Vec<MaintenanceWindow>, StoreError>;

    /// Deletes windows with `ends_at <= before` and returns how many were deleted.
    async fn purge_windows_ended_before(&self, before: Timestamp) -> Result<u64, StoreError>;
}

/// Hygiene advisories.
#[async_trait]
pub trait AdvisoryRepo: Send + Sync {
    /// Inserts an advisory or replaces the one with the same id.
    async fn upsert(&self, advisory: &Advisory) -> Result<(), StoreError>;

    /// Advisories not yet resolved, newest `created_at` first.
    async fn list_open(&self) -> Result<Vec<Advisory>, StoreError>;

    /// Marks an advisory resolved at `at`. Resolving twice keeps the first time.
    async fn resolve(&self, id: AdvisoryId, at: Timestamp) -> Result<(), StoreError>;
}

/// Metric samples (the wire calls them digest facts) contributed by checks and skills.
#[async_trait]
pub trait MetricSampleRepo: Send + Sync {
    /// Appends a fact. Facts are never replaced; the digest picks the latest per key.
    async fn record(&self, fact: &DigestFact) -> Result<(), StoreError>;

    /// Facts for `host` observed in `[from, until)`, oldest first. The caller computes day
    /// boundaries in its configured time zone.
    async fn list(
        &self,
        host: HostId,
        from: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<DigestFact>, StoreError>;

    /// Records that the agent no longer reports `key` for `host`, as of `at`. The samples stay.
    async fn retire(&self, host: HostId, key: &str, at: Timestamp) -> Result<(), StoreError>;

    /// The keys retired for `host` with the time each was retired.
    async fn retired(&self, host: HostId) -> Result<Vec<(String, Timestamp)>, StoreError>;
}

/// The settings key under which a host's retired fact keys are kept.
fn retired_key(host: HostId) -> String {
    format!("facts.retired.{host}")
}

/// Records in `settings` that `key` is no longer reported for `host` as of `at`. A store keeps
/// this beside its samples without a table of its own: the samples stay, only the mark is new.
///
/// # Errors
///
/// Whatever the store returns for the read or the write.
pub async fn retire_via<S: super::SettingsRepo + ?Sized>(
    settings: &S,
    host: HostId,
    key: &str,
    at: Timestamp,
) -> Result<(), StoreError> {
    let name = retired_key(host);
    let mut marks = match settings.get(&name).await? {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    marks.insert(key.to_owned(), serde_json::Value::String(at.to_string()));
    settings.set(&name, &serde_json::Value::Object(marks)).await
}

/// The retired keys of `host` with the time each was retired, sorted by key.
///
/// # Errors
///
/// Whatever the store returns for the read.
pub async fn retired_via<S: super::SettingsRepo + ?Sized>(
    settings: &S,
    host: HostId,
) -> Result<Vec<(String, Timestamp)>, StoreError> {
    let Some(serde_json::Value::Object(marks)) = settings.get(&retired_key(host)).await? else {
        return Ok(Vec::new());
    };
    Ok(marks
        .into_iter()
        .filter_map(|(key, at)| Some((key, at.as_str()?.parse().ok()?)))
        .collect())
}
