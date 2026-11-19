//! Operations: durable pieces of work on hosts, made of immutable steps (ARCHITECTURE.md §5).
//!
//! This module holds the pure part: [`state::OperationState`], the decision function
//! [`decide::decide`] with its events and results, the audit kinds and the journal payload
//! builders. Nothing here does I/O, reads a clock or draws randomness.

pub mod audit;
pub mod decide;
pub mod journal;
pub mod state;

pub use decide::{
    AuditNote, DecideError, Decision, EffectSpec, ExecutionProposal, IdSource, JournalNote,
    NewEdge, NewStep, OperationEvent, ProgressChange, derive_status,
};
pub use state::OperationState;
