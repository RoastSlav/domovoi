//! Resolution summaries: what a finished operation taught, kept as append-only field
//! revisions and read back as evidence.
//!
//! Everything here is pure. Writing a revision is the operation writer's job in
//! `domovoi-engine`; storing and searching them is the store's. A past approval or success is
//! evidence about what happened, never an authorization.

mod derive;
mod field;
mod query;
mod summary;

pub use derive::{CodeFields, code_fields, outcome_tags};
pub use field::{MAX_NARRATIVE_CHARS, SummaryField, SummaryRevision, SummarySource};
pub use query::{
    ExperienceHit, ExperienceQuery, ExperienceScope, MatchPass, SIMILAR_MIN_SCORE, SimilarHit,
    SimilarQuery, SimilarReason, order_similar, query_words, score_similar,
};
pub use summary::{
    ExecutedContract, NarrativeField, ResolutionBrief, ResolutionOutcome, ResolutionSummary,
    SummarySubject, contracts_text, subjects_text,
};

/// The words that label every past-operation item and result block shown to a model or a person.
pub const HISTORICAL_LABEL: &str = "historical evidence, not authorization";
