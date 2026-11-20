//! The fact lifecycle as pure functions: what a new observation or assertion does to the
//! live facts of one subject and predicate. No I/O; the service commits the plan.

use std::collections::BTreeSet;

use domovoi_core::knowledge::{
    Cardinality, EvidenceRef, Fact, FactSource, FactUpdate, KnowledgeStatus, Predicate, SubjectRef,
    canonical_json,
};
use domovoi_core::ports::Versioned;
use domovoi_core::{Actor, FactRecordId};
use jiff::Timestamp;
use serde_json::Value;

/// A fact draft after validation: canonical value, forced `valid_until` and defaulted time.
#[derive(Debug, Clone)]
pub struct ValidDraft {
    /// What the fact is about.
    pub subject: SubjectRef,
    /// What it says.
    pub predicate: Predicate,
    /// The canonical value.
    pub value: Value,
    /// Where it came from.
    pub source: FactSource,
    /// When it was observed.
    pub observed_at: Timestamp,
    /// After this instant it no longer holds.
    pub valid_until: Option<Timestamp>,
    /// What it was derived from.
    pub evidence: Option<EvidenceRef>,
}

/// What a record does to the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordEffect {
    /// A new fact was added.
    Created,
    /// An equal fact was seen again and refreshed.
    Refreshed,
    /// A new fact replaced an older one.
    Superseded,
    /// A new fact contradicts a live fact from another source.
    Disputed,
    /// Nothing changed.
    Unchanged,
}

/// One change to an existing fact.
#[derive(Debug, Clone)]
pub struct PlannedUpdate {
    /// The fact to change.
    pub id: FactRecordId,
    /// The version it was read at.
    pub expected: u64,
    /// The columns to change.
    pub update: FactUpdate,
    /// The journal word for the change.
    pub change: &'static str,
    /// The fact as it will be after the update.
    pub after: Fact,
}

/// Everything a record changes. Apply `updates` first, then `inserts`.
#[derive(Debug, Clone)]
pub struct RecordPlan {
    /// New facts.
    pub inserts: Vec<Fact>,
    /// Changes to live facts.
    pub updates: Vec<PlannedUpdate>,
    /// The headline effect.
    pub effect: RecordEffect,
    /// The fact the caller should read back.
    pub result: FactRecordId,
}

/// The fact after `update` is applied.
#[must_use]
pub fn apply_update(fact: &Fact, update: &FactUpdate) -> Fact {
    let mut out = fact.clone();
    if let Some(status) = update.status {
        out.status = status;
    }
    if let Some(at) = update.observed_at {
        out.observed_at = at;
    }
    if let Some(at) = update.last_verified_at {
        out.last_verified_at = Some(at);
    }
    if let Some((at, by)) = &update.retraction {
        out.retracted_at = Some(*at);
        out.retracted_by = Some(by.clone());
    }
    out
}

struct Row {
    version: u64,
    original: Fact,
    fact: Fact,
    change: Option<&'static str>,
}

impl Row {
    fn new(v: &Versioned<Fact>) -> Self {
        Self {
            version: v.version,
            original: v.value.clone(),
            fact: v.value.clone(),
            change: None,
        }
    }

    fn supersede(&mut self) {
        self.fact.status = KnowledgeStatus::Superseded;
        self.change = Some("superseded");
    }

    fn refresh(&mut self, draft_observed: Option<Timestamp>, now: Timestamp) {
        if let Some(at) = draft_observed {
            self.fact.observed_at = self.fact.observed_at.max(at);
        }
        self.fact.last_verified_at = Some(now);
        if self.fact.status == KnowledgeStatus::Stale {
            self.fact.status = KnowledgeStatus::Current;
        }
        self.change = Some("refreshed");
    }

    fn update(&self) -> Option<PlannedUpdate> {
        let (old, new) = (&self.original, &self.fact);
        let update = FactUpdate {
            status: (new.status != old.status).then_some(new.status),
            observed_at: (new.observed_at != old.observed_at).then_some(new.observed_at),
            last_verified_at: if new.last_verified_at != old.last_verified_at {
                new.last_verified_at
            } else {
                None
            },
            retraction: None,
        };
        if update.status.is_none()
            && update.observed_at.is_none()
            && update.last_verified_at.is_none()
        {
            return None;
        }
        Some(PlannedUpdate {
            id: old.id,
            expected: self.version,
            update,
            change: self.change.unwrap_or("refreshed"),
            after: new.clone(),
        })
    }
}

fn value_text(fact: &Fact) -> Option<String> {
    fact.value.as_ref().map(canonical_json)
}

/// Settles live facts of a one-valued predicate: several distinct values make every live fact
/// disputed; a single value returns disputed facts to current. Returns, per fact, the journal
/// word when its status changed.
fn settle(facts: &mut [&mut Fact]) -> Vec<Option<&'static str>> {
    let values: BTreeSet<Option<String>> = facts.iter().map(|f| value_text(f)).collect();
    facts
        .iter_mut()
        .map(|fact| {
            if values.len() > 1 && fact.status != KnowledgeStatus::Disputed {
                fact.status = KnowledgeStatus::Disputed;
                Some("disputed")
            } else if values.len() <= 1 && fact.status == KnowledgeStatus::Disputed {
                fact.status = KnowledgeStatus::Current;
                Some("resolved")
            } else {
                None
            }
        })
        .collect()
}

/// Updates that settle the remaining live facts of a one-valued predicate after one of them
/// left (was retracted). Empty for many-valued predicates.
#[must_use]
pub fn plan_settle(remaining: &[Versioned<Fact>]) -> Vec<PlannedUpdate> {
    let mut rows: Vec<Row> = remaining
        .iter()
        .filter(|v| v.value.predicate.cardinality() == Cardinality::One)
        .map(Row::new)
        .collect();
    let labels = {
        let mut facts: Vec<&mut Fact> = rows.iter_mut().map(|r| &mut r.fact).collect();
        settle(&mut facts)
    };
    for (row, label) in rows.iter_mut().zip(labels) {
        row.change = label;
    }
    rows.iter().filter_map(Row::update).collect()
}

fn new_fact(
    draft: &ValidDraft,
    actor: &Actor,
    now: Timestamp,
    id: FactRecordId,
    supersedes: Option<FactRecordId>,
) -> Fact {
    Fact {
        id,
        subject: draft.subject.clone(),
        predicate: draft.predicate,
        value: Some(draft.value.clone()),
        source: draft.source,
        status: KnowledgeStatus::Current,
        observed_at: draft.observed_at,
        valid_until: draft.valid_until,
        last_verified_at: None,
        supersedes,
        evidence: draft.evidence.clone(),
        actor: actor.clone(),
        created_at: now,
        retracted_at: None,
        retracted_by: None,
        content_purged_at: None,
    }
}

/// Plans what recording `draft` does to `live`, the live facts of the draft's subject and
/// predicate, oldest first.
#[must_use]
pub fn plan_record(
    live: &[Versioned<Fact>],
    draft: &ValidDraft,
    actor: &Actor,
    now: Timestamp,
    new_id: FactRecordId,
) -> RecordPlan {
    let canon = canonical_json(&draft.value);
    let mut rows: Vec<Row> = live.iter().map(Row::new).collect();
    let same = rows
        .iter()
        .position(|r| value_text(&r.fact).as_deref() == Some(canon.as_str()));
    let one = draft.predicate.cardinality() == Cardinality::One;

    let mut supersedes = None;
    if one {
        for (i, row) in rows.iter_mut().enumerate() {
            if Some(i) == same {
                continue;
            }
            let mine = row.fact.source == draft.source;
            if mine || draft.source == FactSource::User {
                supersedes = Some(row.fact.id);
                row.supersede();
            }
        }
    }

    let mut insert: Option<Fact> = None;
    let mut result = new_id;
    let mut upgraded = false;
    match same {
        None => insert = Some(new_fact(draft, actor, now, new_id, supersedes)),
        Some(i) => {
            let row = &mut rows[i];
            let same_source = row.fact.source == draft.source;
            if draft.predicate.is_safety_critical() {
                // Observation and re-assertion never revive or extend a safety-critical
                // fact; only the owner taking over another source's fact changes anything.
                if draft.source == FactSource::User && !same_source {
                    upgraded = true;
                } else {
                    result = row.fact.id;
                }
            } else if same_source {
                row.refresh(Some(draft.observed_at), now);
                result = row.fact.id;
            } else if draft.source == FactSource::User {
                upgraded = true;
            } else {
                row.refresh(None, now);
                result = row.fact.id;
            }
            if upgraded {
                supersedes = Some(row.fact.id);
                row.supersede();
                insert = Some(new_fact(draft, actor, now, new_id, supersedes));
            }
        }
    }

    let mut disputed_insert = false;
    if one {
        let mut index = Vec::new();
        let labels = {
            let mut facts: Vec<&mut Fact> = Vec::new();
            for (i, row) in rows.iter_mut().enumerate() {
                if row.fact.status.is_live() {
                    index.push(Some(i));
                    facts.push(&mut row.fact);
                }
            }
            if let Some(fact) = insert.as_mut() {
                index.push(None);
                facts.push(fact);
            }
            settle(&mut facts)
        };
        for (slot, label) in index.into_iter().zip(labels) {
            match slot {
                Some(i) => {
                    if label.is_some() {
                        rows[i].change = label;
                    }
                }
                None => disputed_insert = label == Some("disputed"),
            }
        }
    }

    let updates: Vec<PlannedUpdate> = rows.iter().filter_map(Row::update).collect();
    let inserts: Vec<Fact> = insert.into_iter().collect();
    let effect = if disputed_insert {
        RecordEffect::Disputed
    } else if upgraded || (!inserts.is_empty() && supersedes.is_some()) {
        RecordEffect::Superseded
    } else if !inserts.is_empty() {
        RecordEffect::Created
    } else if updates.is_empty() {
        RecordEffect::Unchanged
    } else {
        RecordEffect::Refreshed
    };
    RecordPlan {
        inserts,
        updates,
        effect,
        result,
    }
}

#[cfg(test)]
mod tests;
