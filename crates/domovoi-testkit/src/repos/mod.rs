//! One in-memory store implementing every repository port.

mod accounts;
mod actions;
mod audit;
mod hosts;
mod incidents;
mod knowledge;
mod triage;

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, MutexGuard};

use domovoi_core::ports::{
    AuditEvent, EnrollmentToken, IncidentTimelineEntry, Page, Paged, StoreError,
};
use domovoi_core::{
    AckRule, AckRuleId, ActionId, ActionRequest, Advisory, AdvisoryId, AgentId, AgentIdentity,
    BaselineEntry, DigestFact, Fingerprint, Host, HostId, Incident, IncidentId, MaintenanceWindow,
    Note, NoteId, Session, Signal, SignalId, TriageReport, User, UserId,
};
use jiff::Timestamp;
use serde_json::Value;

use crate::lock;

/// A usage record: provider, tokens in, tokens out, cost, time.
type UsageRow = (String, u64, u64, u64, Timestamp);

#[derive(Default)]
struct State {
    hosts: BTreeMap<HostId, Host>,
    agents: BTreeMap<AgentId, AgentIdentity>,
    tokens: BTreeMap<[u8; 32], EnrollmentToken>,
    signals: BTreeMap<SignalId, (Signal, Option<IncidentId>)>,
    incidents: BTreeMap<IncidentId, Incident>,
    timeline: BTreeMap<IncidentId, Vec<IncidentTimelineEntry>>,
    actions: BTreeMap<ActionId, ActionRequest>,
    audit: Vec<AuditEvent>,
    notes: BTreeMap<NoteId, Note>,
    baselines: Vec<BaselineEntry>,
    ack_rules: BTreeMap<AckRuleId, AckRule>,
    windows: Vec<MaintenanceWindow>,
    advisories: BTreeMap<AdvisoryId, Advisory>,
    facts: Vec<DigestFact>,
    triage_cache: BTreeMap<(Fingerprint, [u8; 32]), (TriageReport, Timestamp)>,
    usage: Vec<UsageRow>,
    settings: BTreeMap<String, Value>,
    users: BTreeMap<UserId, User>,
    sessions: BTreeMap<[u8; 32], Session>,
}

/// An in-memory implementation of every repository port in `domovoi_core::ports`.
///
/// It follows the documented semantics of each port, so the contract suites in
/// [`contracts`](crate::contracts) pass against it. [`fail_next`](Self::fail_next) makes the
/// next call fail, for testing error paths.
#[derive(Default)]
pub struct InMemoryStore {
    state: Mutex<State>,
    failures: Mutex<VecDeque<StoreError>>,
}

impl InMemoryStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next repository call, whatever it is, fail with `error` without changing
    /// anything.
    pub fn fail_next(&self, error: StoreError) {
        lock(&self.failures).push_back(error);
    }

    /// Takes the next injected failure or locks the state.
    fn begin(&self) -> Result<MutexGuard<'_, State>, StoreError> {
        if let Some(error) = lock(&self.failures).pop_front() {
            return Err(error);
        }
        Ok(lock(&self.state))
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
