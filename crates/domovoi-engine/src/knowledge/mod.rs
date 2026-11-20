//! The knowledge service: facts, relations and entities with a lifecycle (source, freshness,
//! disputes, retraction), management paths and their owner confirmations, and the knowledge
//! layer of model context. Pure decisions live in [`decide`]; everything commits through the
//! unit of work.

mod coverage;
pub mod decide;
mod error;
mod graph;
mod owner;
mod paths;
mod service;
mod source;
mod sweep;

pub use coverage::UnavailableCoverage;
pub use decide::RecordEffect;
pub use error::KnowledgeError;
pub use graph::RelationDraft;
pub use paths::{ManagementPathView, NetworkChangeCause};
pub use service::{FactDraft, KnowledgeDeps, KnowledgeService, Recorded};
pub use source::KnowledgeSource;
pub use sweep::SweepReport;
