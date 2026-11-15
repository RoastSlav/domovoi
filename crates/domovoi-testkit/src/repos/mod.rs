//! One in-memory store implementing every repository port.

mod accounts;
mod actions;
mod admission;
mod audit;
mod health;
mod hosts;
mod incidents;
mod knowledge;
mod notify;
mod triage;
mod uow;
mod uow_apply;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Mutex, MutexGuard};

use domovoi_core::ports::{
    AdmissionPurpose, AggregateRef, AuditEvent, CallOutcome, EffectRecord, EnrollmentToken,
    IncidentTimelineEntry, JournalEntry, Page, Paged, StoreError, UsageSource,
};
use domovoi_core::{
    AckRule, AckRuleId, ActionId, ActionRequest, Advisory, AdvisoryId, AgentId, AgentIdentity,
    AgentReport, BaselineEntry, DigestFact, Fingerprint, Host, HostId, Incident, IncidentId,
    MaintenanceWindow, Note, NoteId, Session, Signal, SignalId, TriageReport, User, UserId,
};
use jiff::Timestamp;
use serde_json::Value;

use crate::lock;

/// One stored model call, as the in-memory usage repository keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRow {
    /// Provider id.
    pub provider: String,
    /// Configured model name, when known.
    pub model: Option<String>,
    /// How the call ended.
    pub outcome: CallOutcome,
    /// Prompt tokens.
    pub tokens_in: u64,
    /// Generated tokens.
    pub tokens_out: u64,
    /// Cost in millionths of a dollar; `None` when the price is unknown.
    pub cost_micros: Option<u64>,
    /// Whether the response's usage was reported, estimated or unknown; `None` for a call
    /// that produced no response.
    pub usage_source: Option<UsageSource>,
    /// How long the call took, in milliseconds.
    pub latency_ms: u64,
    /// When the call started.
    pub at: Timestamp,
}

#[derive(Default, Clone)]
struct State {
    hosts: BTreeMap<HostId, Host>,
    agents: BTreeMap<AgentId, AgentIdentity>,
    reports: BTreeMap<AgentId, AgentReport>,
    tokens: BTreeMap<[u8; 32], EnrollmentToken>,
    signals: BTreeMap<SignalId, (Signal, Option<IncidentId>)>,
    incidents: BTreeMap<IncidentId, Incident>,
    timeline: BTreeMap<IncidentId, Vec<IncidentTimelineEntry>>,
    actions: BTreeMap<ActionId, ActionRequest>,
    audit: Vec<AuditEvent>,
    notes: BTreeMap<NoteId, Note>,
    versions: BTreeMap<AggregateRef, u64>,
    journal: Vec<JournalEntry>,
    effects: BTreeMap<domovoi_core::EffectId, EffectRecord>,
    /// Effect ids in insertion order; `effects` keeps lookup by id.
    effect_order: Vec<domovoi_core::EffectId>,
    baselines: Vec<BaselineEntry>,
    ack_rules: BTreeMap<AckRuleId, AckRule>,
    windows: Vec<MaintenanceWindow>,
    advisories: BTreeMap<AdvisoryId, Advisory>,
    facts: Vec<DigestFact>,
    triage_cache: BTreeMap<(Fingerprint, [u8; 32]), (TriageReport, Timestamp)>,
    usage: Vec<UsageRow>,
    admissions: Vec<(AdmissionPurpose, Timestamp)>,
    settings: BTreeMap<String, Value>,
    users: BTreeMap<UserId, User>,
    sessions: BTreeMap<[u8; 32], Session>,
    notify: BTreeMap<IncidentId, (domovoi_core::NotifyState, Timestamp)>,
}

/// An in-memory implementation of every repository port in `domovoi_core::ports`.
///
/// It follows the documented semantics of each port, so the contract suites in
/// [`contracts`](crate::contracts) pass against it. [`fail_next`](Self::fail_next) makes the
/// next call fail, for testing error paths.
///
/// The audit log here is a plain list: it does not compute the hash chain that the SQLite store
/// writes, and it has no `verify_audit`.
#[derive(Default)]
pub struct InMemoryStore {
    state: Mutex<State>,
    failures: Mutex<VecDeque<StoreError>>,
    /// How long an incident listing or note search takes before it answers.
    delay: Mutex<Option<std::time::Duration>>,
}

impl InMemoryStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes incident listings and note searches take `delay` before answering, so a test
    /// can put them past a run deadline without a real sleep.
    #[must_use]
    pub fn with_delay(self, delay: std::time::Duration) -> Self {
        *lock(&self.delay) = Some(delay);
        self
    }

    /// Waits the configured delay, if any. The guard is dropped before the wait.
    async fn pause(&self) {
        let delay = *lock(&self.delay);
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
    }

    /// Makes the next repository call, whatever it is, fail with `error` without changing
    /// anything.
    pub fn fail_next(&self, error: StoreError) {
        lock(&self.failures).push_back(error);
    }

    /// Every stored model call, oldest first.
    #[must_use]
    pub fn usage_rows(&self) -> Vec<UsageRow> {
        lock(&self.state).usage.clone()
    }

    /// Takes the next injected failure or locks the state.
    fn begin(&self) -> Result<MutexGuard<'_, State>, StoreError> {
        if let Some(error) = lock(&self.failures).pop_front() {
            return Err(error);
        }
        Ok(lock(&self.state))
    }

    /// The incident each stored reference names: timeline entries per incident, signal links per
    /// incident, and the incidents that exist.
    ///
    /// Test-only support for the crash invariants. The repository ports read a timeline and its
    /// signal links only for an incident that exists, so a reference left behind by a vanished
    /// incident is invisible through them and has to be read here.
    pub(crate) fn integrity_refs(
        &self,
    ) -> (
        BTreeMap<IncidentId, usize>,
        BTreeMap<IncidentId, usize>,
        BTreeSet<IncidentId>,
    ) {
        let state = lock(&self.state);
        let timeline = state
            .timeline
            .iter()
            .filter(|(_, entries)| !entries.is_empty())
            .map(|(incident, entries)| (*incident, entries.len()))
            .collect();
        let mut links = BTreeMap::new();
        for (_, incident) in state.signals.values() {
            if let Some(incident) = incident {
                *links.entry(*incident).or_insert(0) += 1;
            }
        }
        (timeline, links, state.incidents.keys().copied().collect())
    }

    /// Appends a timeline entry for an incident that may not exist, so the crash invariants'
    /// orphan check can be exercised. [`IncidentRepo::append_event`] refuses this.
    #[cfg(test)]
    pub(crate) fn insert_dangling_timeline(&self, entry: &IncidentTimelineEntry) {
        lock(&self.state)
            .timeline
            .entry(entry.incident)
            .or_default()
            .push(entry.clone());
    }
}

/// Sort key for paged listings: time in nanoseconds (or a sequence number), then id.
type Key = (i128, String);

fn encode_cursor(key: &Key) -> String {
    format!("{}:{}", key.0, key.1)
}

fn decode_cursor(cursor: &str) -> Result<Key, StoreError> {
    let (time, id) = cursor
        .split_once(':')
        .ok_or_else(|| StoreError::Backend("invalid cursor".to_owned()))?;
    let time = time
        .parse()
        .map_err(|_| StoreError::Backend("invalid cursor".to_owned()))?;
    Ok((time, id.to_owned()))
}

/// Pages `items` newest first by key, with a keyset cursor.
fn paginate<T>(mut items: Vec<(Key, T)>, page: &Page) -> Result<Paged<T>, StoreError> {
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let after = page.cursor.as_deref().map(decode_cursor).transpose()?;
    let limit = page.effective_limit();
    let mut rest: Vec<(Key, T)> = items
        .into_iter()
        .filter(|(key, _)| after.as_ref().is_none_or(|cursor| key < cursor))
        .collect();
    let more = rest.len() > limit;
    rest.truncate(limit);
    let next_cursor = if more {
        rest.last().map(|(key, _)| encode_cursor(key))
    } else {
        None
    };
    Ok(Paged {
        items: rest.into_iter().map(|(_, item)| item).collect(),
        next_cursor,
    })
}

/// Lowercased runs of letters and digits, the way the SQLite FTS5 tokenizer splits text.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How many of `haystack`'s words match `query`'s words, or `None` unless every query word
/// is present. An empty query matches nothing.
fn word_score(query: &[String], haystack: &[String]) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let mut score = 0;
    for word in query {
        let hits = haystack.iter().filter(|w| *w == word).count();
        if hits == 0 {
            return None;
        }
        score += hits;
    }
    Some(score)
}

fn conflict(what: &str) -> StoreError {
    StoreError::Conflict(what.to_owned())
}

fn limit(limit: u16) -> usize {
    Page::first(limit).effective_limit()
}

/// The stored version of an aggregate; a row written before versions existed is 1.
pub(in crate::repos) fn version(state: &State, aggregate: &AggregateRef) -> u64 {
    state.versions.get(aggregate).copied().unwrap_or(1)
}

/// Stores a new version for an aggregate.
pub(in crate::repos) fn set_version(state: &mut State, aggregate: AggregateRef, next: u64) {
    state.versions.insert(aggregate, next);
}

/// Raises an aggregate's version by one and returns the new value.
pub(in crate::repos) fn bump_version(state: &mut State, aggregate: AggregateRef) -> u64 {
    let next = version(state, &aggregate).saturating_add(1);
    set_version(state, aggregate, next);
    next
}
