//! The in-memory resolution summaries: the same write rules as the SQLite store, and reads that
//! rank with the core pure functions so the two cannot drift.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use domovoi_core::experience::{
    ExperienceHit, ExperienceQuery, ExperienceScope, MatchPass, ResolutionBrief, ResolutionSummary,
    SIMILAR_MIN_SCORE, SimilarHit, SimilarQuery, SummaryField, SummaryRevision, contracts_text,
    order_similar, query_words, score_similar, subjects_text,
};
use domovoi_core::ports::{
    AggregateRef, CommitError, Expectation, ExperienceRead, Observed, StoreError, SummaryFieldWrite,
};
use domovoi_core::{HostId, OperationId};

use super::{InMemoryStore, State};

/// Every revision of one operation, and the outcome words of its newest code write.
#[derive(Default, Clone)]
pub(super) struct SummaryState {
    revisions: Vec<SummaryRevision>,
    tags: Vec<String>,
}

fn refuse(message: &str) -> CommitError {
    CommitError::Constraint(message.to_owned())
}

/// Appends one revision with the store's rules.
pub(super) fn apply(state: &mut State, write: &SummaryFieldWrite) -> Result<(), CommitError> {
    if !write.source.may_write(write.field) {
        return Err(refuse("source cannot write this field"));
    }
    let finished = state
        .operations
        .get(&write.operation)
        .is_some_and(|op| op.status.is_terminal() && op.ended_at.is_some());
    if !finished {
        return Err(refuse("a summary needs a finished operation"));
    }
    let stored = state.summaries.get(&write.operation);
    if stored.is_none_or(|e| e.revisions.is_empty()) && write.field != SummaryField::Outcome {
        return Err(refuse("the first summary revision is the outcome"));
    }
    let newest = stored
        .map(|e| e.revisions.iter())
        .into_iter()
        .flatten()
        .filter(|r| r.field == write.field)
        .map(|r| r.revision)
        .max()
        .unwrap_or(0);
    if newest != write.expected_revision {
        return Err(CommitError::Conflict {
            aggregate: AggregateRef::operation(write.operation),
            expected: Expectation::Version(u64::from(write.expected_revision)),
            found: Observed::Version(u64::from(newest)),
        });
    }
    if !write.field.accepts(&write.value) {
        return Err(refuse("bad summary value"));
    }
    let entry = state.summaries.entry(write.operation).or_default();
    entry.revisions.push(SummaryRevision {
        operation: write.operation,
        field: write.field,
        revision: newest + 1,
        value: write.value.clone(),
        source: write.source,
        actor: write.actor.clone(),
        created_at: write.at,
    });
    if let Some(tags) = &write.tags {
        entry.tags.clone_from(tags);
    }
    Ok(())
}

fn field_rank(field: SummaryField) -> usize {
    SummaryField::ALL
        .iter()
        .position(|f| *f == field)
        .unwrap_or(0)
}

fn current_of(entry: &SummaryState) -> Vec<SummaryRevision> {
    let mut newest: BTreeMap<SummaryField, &SummaryRevision> = BTreeMap::new();
    for revision in &entry.revisions {
        let slot = newest.entry(revision.field).or_insert(revision);
        if revision.revision > slot.revision {
            *slot = revision;
        }
    }
    let mut out: Vec<SummaryRevision> = newest.into_values().cloned().collect();
    out.sort_by_key(|r| field_rank(r.field));
    out
}

fn summary_of(state: &State, id: OperationId) -> Option<ResolutionSummary> {
    let entry = state.summaries.get(&id)?;
    let operation = state.operations.get(&id)?;
    ResolutionSummary::fold(&operation.goal, operation.ended_at?, &current_of(entry))
        .ok()
        .flatten()
}

/// The words the full-text index holds for one summary.
fn indexed_words(state: &State, summary: &ResolutionSummary, columns: &[&str]) -> BTreeSet<String> {
    let text = |n: &Option<domovoi_core::experience::NarrativeField>| {
        n.as_ref().map(|f| f.text.clone()).unwrap_or_default()
    };
    let tags = state
        .summaries
        .get(&summary.operation)
        .map(|e| e.tags.join(" "))
        .unwrap_or_default();
    let all = [
        ("goal", summary.goal.clone()),
        ("problem", text(&summary.problem)),
        ("cause", text(&summary.cause)),
        ("fix", text(&summary.fix)),
        ("subjects", subjects_text(&summary.subjects)),
        ("contracts", contracts_text(&summary.contracts)),
        ("tags", tags),
    ];
    all.into_iter()
        .filter(|(name, _)| columns.contains(name))
        .flat_map(|(_, t)| tokens(&t))
        .collect()
}

/// The lowercase alphanumeric words of `text`, as the full-text tokenizer sees them.
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn in_scope(summary: &ResolutionSummary, host: Option<HostId>) -> bool {
    host.is_none_or(|h| summary.hosts == [h])
}

const ALL_COLUMNS: [&str; 7] = [
    "goal",
    "problem",
    "cause",
    "fix",
    "subjects",
    "contracts",
    "tags",
];

fn newest_first(summaries: &mut [ResolutionSummary]) {
    summaries.sort_by(|a, b| {
        b.ended_at
            .cmp(&a.ended_at)
            .then_with(|| b.operation.cmp(&a.operation))
    });
}

#[async_trait]
impl ExperienceRead for InMemoryStore {
    async fn summary(
        &self,
        operation: OperationId,
    ) -> Result<Option<ResolutionSummary>, StoreError> {
        let state = self.begin()?;
        Ok(summary_of(&state, operation))
    }

    async fn revisions(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        let state = self.begin()?;
        let mut all = state
            .summaries
            .get(&operation)
            .map(|e| e.revisions.clone())
            .unwrap_or_default();
        all.sort_by_key(|r| (field_rank(r.field), r.revision));
        Ok(all)
    }

    async fn current(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .summaries
            .get(&operation)
            .map(current_of)
            .unwrap_or_default())
    }

    async fn briefs(
        &self,
        operations: &[OperationId],
    ) -> Result<BTreeMap<OperationId, ResolutionBrief>, StoreError> {
        let state = self.begin()?;
        Ok(operations
            .iter()
            .filter_map(|id| {
                let s = summary_of(&state, *id)?;
                Some((
                    *id,
                    ResolutionBrief {
                        outcome: s.outcome,
                        verified: s.verified,
                        ended_at: s.ended_at,
                    },
                ))
            })
            .collect())
    }

    async fn search(&self, query: &ExperienceQuery) -> Result<Vec<ExperienceHit>, StoreError> {
        let state = self.begin()?;
        let host = match &query.scope {
            ExperienceScope::Host(h) => Some(*h),
            ExperienceScope::Global => None,
        };
        let mut candidates: Vec<ResolutionSummary> = state
            .summaries
            .keys()
            .filter(|id| Some(**id) != query.exclude)
            .filter_map(|id| summary_of(&state, *id))
            .filter(|s| in_scope(s, host))
            .collect();
        newest_first(&mut candidates);
        let words = query_words(&query.text);
        let limit = query.limit();
        if words.is_empty() {
            return Ok(candidates
                .into_iter()
                .take(limit)
                .map(|summary| ExperienceHit {
                    summary,
                    pass: MatchPass::AllWords,
                    snippet: None,
                })
                .collect());
        }
        let mut all = Vec::new();
        let mut any = Vec::new();
        for summary in candidates {
            let have = indexed_words(&state, &summary, &ALL_COLUMNS);
            let hits = words.iter().filter(|w| have.contains(*w)).count();
            if hits == words.len() {
                all.push(summary);
            } else if hits > 0 {
                any.push(summary);
            }
        }
        let pass = |list: Vec<ResolutionSummary>, pass| {
            list.into_iter().map(move |summary| ExperienceHit {
                summary,
                pass,
                snippet: None,
            })
        };
        Ok(pass(all, MatchPass::AllWords)
            .chain(pass(any, MatchPass::AnyWord))
            .take(limit)
            .collect())
    }

    async fn similar(&self, query: &SimilarQuery) -> Result<Vec<SimilarHit>, StoreError> {
        let state = self.begin()?;
        let mut candidates: Vec<ResolutionSummary> = state
            .summaries
            .keys()
            .filter(|id| Some(**id) != query.exclude)
            .filter_map(|id| summary_of(&state, *id))
            .filter(|s| in_scope(s, Some(query.host)))
            .collect();
        newest_first(&mut candidates);
        let words = query_words(&query.text);
        let columns = ["goal", "problem", "cause"];
        let ranked: Vec<OperationId> = candidates
            .iter()
            .filter(|s| {
                let have = indexed_words(&state, s, &columns);
                words.iter().any(|w| have.contains(w))
            })
            .take(20)
            .map(|s| s.operation)
            .collect();
        let mut hits: Vec<SimilarHit> = candidates
            .into_iter()
            .filter_map(|summary| {
                let by_subject = summary.subjects.iter().any(|s| {
                    s.host == query.host
                        && (query.check.as_ref() == Some(&s.check)
                            || (query.target.is_some() && s.target == query.target))
                });
                let rank = ranked.iter().position(|id| *id == summary.operation);
                if !by_subject && rank.is_none() {
                    return None;
                }
                let (score, reasons) = score_similar(query, &summary, rank);
                (score >= SIMILAR_MIN_SCORE).then_some(SimilarHit {
                    summary,
                    score,
                    reasons,
                })
            })
            .collect();
        order_similar(&mut hits);
        hits.truncate(query.limit());
        Ok(hits)
    }

    async fn undrafted(&self, limit: u16) -> Result<Vec<OperationId>, StoreError> {
        let state = self.begin()?;
        let mut open: Vec<_> = state
            .operations
            .values()
            .filter(|op| op.status.is_terminal() && !state.summaries.contains_key(&op.id))
            .map(|op| (op.ended_at, op.id))
            .collect();
        open.sort();
        Ok(open
            .into_iter()
            .take(usize::from(limit))
            .map(|(_, id)| id)
            .collect())
    }
}
