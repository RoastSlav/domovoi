//! Context for the model: sources of items and the aggregator that layers them.
//!
//! Each source implements the [`ContextSource`](domovoi_core::ports::ContextSource) port: it
//! answers a query with redacted, bounded items that carry their provenance and a status, and
//! says so with a gap when it cannot answer, so a failed read is never mistaken for an empty
//! one. The [`Aggregator`] asks the sources, re-checks what they return, removes duplicates,
//! applies per-layer and total size caps and renders the result in a fixed layer order, naming
//! everything it left out. A source only reads; nothing here can approve, run or store anything.

mod aggregate;
mod evidence;
mod guide;
mod layout;
mod normalize;
mod plan;
mod render;
mod shrink;
pub(crate) mod sources;

pub use aggregate::Aggregator;
pub use evidence::EvidenceSource;
pub use guide::CONTEXT_GUIDE;
pub use layout::{Omission, Section, title_of};
pub use plan::{ContextBudget, ContextPlan, ContextPurpose, LayerCaps};
pub use render::LayeredContext;
