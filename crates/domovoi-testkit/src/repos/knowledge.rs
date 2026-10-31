use async_trait::async_trait;
use domovoi_core::ports::{
    AckRepo, AdvisoryRepo, BaselineRepo, FactRepo, NoteHit, NoteRepo, StoreError,
};
use domovoi_core::{
    AckRule, AckRuleId, Advisory, AdvisoryId, BaselineEntry, BaselineKind, DigestFact, HostId,
    MaintenanceWindow, Note, NoteId,
};
use jiff::Timestamp;

use super::{InMemoryStore, limit, word_score, words};

/// Longest snippet returned by note search, in characters.
const SNIPPET_CHARS: usize = 160;

fn note_words(note: &Note) -> Vec<String> {
    let mut all = words(&note.title);
    all.extend(words(&note.body_markdown));
    for tag in &note.tags {
        all.extend(words(tag));
    }
    all
}

/// The first body line containing a query word, or the title.
fn snippet(note: &Note, query: &[String]) -> String {
    let line = note
        .body_markdown
        .lines()
        .find(|line| words(line).iter().any(|w| query.contains(w)))
        .unwrap_or(&note.title);
    let line = line.trim();
    let line = if line.is_empty() { "…" } else { line };
    line.chars().take(SNIPPET_CHARS).collect()
}

const fn kind_rank(kind: BaselineKind) -> u8 {
    match kind {
        BaselineKind::Container => 0,
        BaselineKind::Unit => 1,
        BaselineKind::Port => 2,
    }
}

#[async_trait]
impl NoteRepo for InMemoryStore {
    async fn upsert(&self, note: &Note) -> Result<(), StoreError> {
        self.begin()?.notes.insert(note.id, note.clone());
        Ok(())
    }

    async fn get(&self, id: NoteId) -> Result<Option<Note>, StoreError> {
        Ok(self.begin()?.notes.get(&id).cloned())
    }

    async fn delete(&self, id: NoteId) -> Result<(), StoreError> {
        self.begin()?
            .notes
            .remove(&id)
            .map(|_| ())
            .ok_or(StoreError::NotFound)
    }

    async fn list(&self, host: Option<HostId>) -> Result<Vec<Note>, StoreError> {
        let mut notes: Vec<Note> = self
            .begin()?
            .notes
            .values()
            .filter(|n| n.host == host)
            .cloned()
            .collect();
        notes.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(notes)
    }

    async fn search(
        &self,
        query: &str,
        host: Option<HostId>,
        max: u16,
    ) -> Result<Vec<NoteHit>, StoreError> {
        let query = words(query);
        let state = self.begin()?;
        let mut hits: Vec<NoteHit> = state
            .notes
            .values()
            .filter(|n| host.is_none() || n.host.is_none() || n.host == host)
            .filter_map(|note| {
                let score = word_score(&query, &note_words(note))?;
                Some(NoteHit {
                    snippet: snippet(note, &query),
                    note: note.clone(),
                    score: score as f64,
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| b.note.updated_at.cmp(&a.note.updated_at))
        });
        hits.truncate(limit(max));
        Ok(hits)
    }
}

#[async_trait]
impl BaselineRepo for InMemoryStore {
    async fn upsert(&self, entry: &BaselineEntry) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        state
            .baselines
            .retain(|e| !(e.host == entry.host && e.kind == entry.kind && e.key == entry.key));
        state.baselines.push(entry.clone());
        Ok(())
    }

    async fn list(&self, host: HostId) -> Result<Vec<BaselineEntry>, StoreError> {
        let mut entries: Vec<BaselineEntry> = self
            .begin()?
            .baselines
            .iter()
            .filter(|e| e.host == host)
            .cloned()
            .collect();
        entries.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| kind_rank(a.kind).cmp(&kind_rank(b.kind)))
        });
        Ok(entries)
    }

    async fn delete(&self, host: HostId, kind: BaselineKind, key: &str) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let before = state.baselines.len();
        state
            .baselines
            .retain(|e| !(e.host == host && e.kind == kind && e.key == key));
        if state.baselines.len() == before {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }
}

#[async_trait]
impl AckRepo for InMemoryStore {
    async fn put_rule(&self, rule: &AckRule) -> Result<(), StoreError> {
        self.begin()?.ack_rules.insert(rule.id, rule.clone());
        Ok(())
    }

    async fn delete_rule(&self, id: AckRuleId) -> Result<(), StoreError> {
        self.begin()?
            .ack_rules
            .remove(&id)
            .map(|_| ())
            .ok_or(StoreError::NotFound)
    }

    async fn list_rules(&self) -> Result<Vec<AckRule>, StoreError> {
        Ok(self.begin()?.ack_rules.values().cloned().collect())
    }

    async fn put_window(&self, window: &MaintenanceWindow) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if !state.windows.contains(window) {
            state.windows.push(window.clone());
        }
        Ok(())
    }

    async fn windows_for(&self, host: HostId) -> Result<Vec<MaintenanceWindow>, StoreError> {
        let mut windows: Vec<MaintenanceWindow> = self
            .begin()?
            .windows
            .iter()
            .filter(|w| w.host == host)
            .cloned()
            .collect();
        windows.sort_by(|a, b| {
            a.starts_at
                .cmp(&b.starts_at)
                .then_with(|| a.ends_at.cmp(&b.ends_at))
        });
        Ok(windows)
    }

    async fn purge_windows_ended_before(&self, before: Timestamp) -> Result<u64, StoreError> {
        let mut state = self.begin()?;
        let count = state.windows.len();
        state.windows.retain(|w| w.ends_at > before);
        Ok((count - state.windows.len()) as u64)
    }
}

#[async_trait]
impl AdvisoryRepo for InMemoryStore {
    async fn upsert(&self, advisory: &Advisory) -> Result<(), StoreError> {
        self.begin()?
            .advisories
            .insert(advisory.id, advisory.clone());
        Ok(())
    }

    async fn list_open(&self) -> Result<Vec<Advisory>, StoreError> {
        let mut open: Vec<Advisory> = self
            .begin()?
            .advisories
            .values()
            .filter(|a| a.resolved_at.is_none())
            .cloned()
            .collect();
        open.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(open)
    }

    async fn resolve(&self, id: AdvisoryId, at: Timestamp) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let advisory = state.advisories.get_mut(&id).ok_or(StoreError::NotFound)?;
        advisory.resolved_at.get_or_insert(at);
        Ok(())
    }
}

#[async_trait]
impl FactRepo for InMemoryStore {
    async fn record(&self, fact: &DigestFact) -> Result<(), StoreError> {
        self.begin()?.facts.push(fact.clone());
        Ok(())
    }

    async fn list(
        &self,
        host: HostId,
        from: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<DigestFact>, StoreError> {
        let mut facts: Vec<DigestFact> = self
            .begin()?
            .facts
            .iter()
            .filter(|f| f.host == host && from <= f.observed_at && f.observed_at < until)
            .cloned()
            .collect();
        facts.sort_by_key(|f| f.observed_at);
        Ok(facts)
    }
}
