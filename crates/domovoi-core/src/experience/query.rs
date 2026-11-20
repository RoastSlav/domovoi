//! Search and similarity over finished operations, as pure functions.
//!
//! The store and the in-memory twin both call these, so ranking cannot drift between them.

use crate::id::{HostId, OperationId};
use crate::signal::{CheckKey, TargetKey};

use super::summary::ResolutionSummary;

/// Where a search may look.
///
/// `Global` is built in three production places only: the owner's search endpoint, the global
/// search of the owner's own Operator conversation, and the store and twin that implement the
/// port. A model can never widen its own scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExperienceScope {
    /// Operations whose host set is exactly this host.
    Host(HostId),
    /// Operations on every host.
    Global,
}

/// A full-text search over finished operations.
#[derive(Debug, Clone)]
pub struct ExperienceQuery {
    /// Where to look.
    pub scope: ExperienceScope,
    /// What to look for; no words lists the newest operations.
    pub text: String,
    /// At most this many hits, clamped to 1 to 50.
    pub limit: u16,
    /// An operation that must not be returned.
    pub exclude: Option<OperationId>,
}

impl ExperienceQuery {
    /// `limit` clamped to 1 to 50.
    #[must_use]
    pub fn limit(&self) -> usize {
        usize::from(self.limit.clamp(1, 50))
    }
}

/// Which pass of the two-pass match found a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPass {
    /// Every word matched.
    AllWords,
    /// At least one word matched.
    AnyWord,
}

impl MatchPass {
    /// `all_words` or `any_word`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AllWords => "all_words",
            Self::AnyWord => "any_word",
        }
    }
}

/// One search hit.
#[derive(Debug, Clone, PartialEq)]
pub struct ExperienceHit {
    /// The operation's current summary.
    pub summary: ResolutionSummary,
    /// Which pass found it.
    pub pass: MatchPass,
    /// A short excerpt around the match, when there was one.
    pub snippet: Option<String>,
}

/// A request for past operations that resemble a situation.
#[derive(Debug, Clone)]
pub struct SimilarQuery {
    /// The host; candidates are fetched for this host only.
    pub host: HostId,
    /// The check to match.
    pub check: Option<CheckKey>,
    /// The target to match.
    pub target: Option<TargetKey>,
    /// Words to match against the goal and narrative.
    pub text: String,
    /// Contract ids the new situation may run.
    pub contracts: Vec<String>,
    /// An operation that must not be returned.
    pub exclude: Option<OperationId>,
    /// At most this many hits, clamped to 1 to 20.
    pub limit: u16,
}

impl SimilarQuery {
    /// `limit` clamped to 1 to 20.
    #[must_use]
    pub fn limit(&self) -> usize {
        usize::from(self.limit.clamp(1, 20))
    }
}

/// Why a past operation was judged similar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimilarReason {
    /// The same check.
    SameCheck,
    /// The same target.
    SameTarget,
    /// It ran this contract.
    SameContract(String),
    /// Its text matches the query words.
    SimilarWords,
}

/// A similar past operation.
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarHit {
    /// The operation's current summary.
    pub summary: ResolutionSummary,
    /// The score; higher is more similar.
    pub score: u32,
    /// Why it scored.
    pub reasons: Vec<SimilarReason>,
}

/// The lowest score a candidate needs to be returned.
pub const SIMILAR_MIN_SCORE: u32 = 4;

/// Scores `candidate` against `query`. `words_rank` is the candidate's zero-based position in
/// the any-word text matches, `None` when absent.
#[must_use]
pub fn score_similar(
    query: &SimilarQuery,
    candidate: &ResolutionSummary,
    words_rank: Option<usize>,
) -> (u32, Vec<SimilarReason>) {
    let mut score = 0;
    let mut reasons = Vec::new();
    if let Some(check) = &query.check
        && candidate.subjects.iter().any(|s| &s.check == check)
    {
        score += 8;
        reasons.push(SimilarReason::SameCheck);
    }
    if let Some(target) = &query.target
        && candidate
            .subjects
            .iter()
            .any(|s| s.target.as_ref() == Some(target))
    {
        score += 4;
        reasons.push(SimilarReason::SameTarget);
    }
    let mut counted: Vec<&str> = Vec::new();
    for id in &query.contracts {
        if counted.len() == 3 || counted.contains(&id.as_str()) {
            continue;
        }
        if candidate.contracts.iter().any(|c| &c.contract == id) {
            counted.push(id);
            score += 2;
            reasons.push(SimilarReason::SameContract(id.clone()));
        }
    }
    let words = match words_rank {
        Some(0..=4) => 4,
        Some(5..=19) => 2,
        _ => 0,
    };
    if words > 0 {
        score += words;
        reasons.push(SimilarReason::SimilarWords);
    }
    (score, reasons)
}

/// Orders hits: score descending, then newer `ended_at`, then larger operation id.
pub fn order_similar(hits: &mut [SimilarHit]) {
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.summary.ended_at.cmp(&a.summary.ended_at))
            .then_with(|| b.summary.operation.cmp(&a.summary.operation))
    });
}

/// Distinct lowercase alphanumeric words of `text` in first-seen order, at most 12, each at
/// most 40 characters. Operators and punctuation vanish.
#[must_use]
pub fn query_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        let word: String = raw.to_lowercase().chars().take(40).collect();
        if !words.contains(&word) {
            words.push(word);
            if words.len() == 12 {
                break;
            }
        }
    }
    words
}
