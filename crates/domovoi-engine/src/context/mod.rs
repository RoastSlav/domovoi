//! Read-only sources of context items for the model.
//!
//! Each source implements the [`ContextSource`](domovoi_core::ports::ContextSource) port: it
//! answers a query with redacted, bounded items that carry their provenance and a status, and
//! says so with a gap when it cannot answer, so a failed read is never mistaken for an empty
//! one. A source only reads; nothing here can approve, run or store anything.

mod evidence;

pub use evidence::EvidenceSource;
