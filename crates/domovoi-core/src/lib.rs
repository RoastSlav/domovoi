//! Domain types, state machines and ports (traits) for Domovoi.
//!
//! This crate is pure: no I/O, no async runtime, no adapters. Everything else
//! in the workspace depends on it. See `ARCHITECTURE.md` §5 (domain model) and
//! §6 (ports).

pub mod error;
pub mod fingerprint;
pub mod history;
pub mod host;
pub mod id;
pub mod incident;
pub mod severity;
pub mod signal;
pub mod verdict;

pub use error::{TransitionError, ValidationError};
pub use fingerprint::Fingerprint;
pub use history::EntityHistory;
pub use host::{Host, HostKind, HostName};
pub use id::{ActionId, AgentId, HostId, IncidentId, NoteId, RequestId, SignalId, UserId};
pub use incident::{
    Incident, IncidentChange, IncidentEvent, IncidentPolicy, IncidentStatus, apply,
};
pub use severity::Severity;
pub use signal::{
    CheckKey, Evidence, EvidenceValue, Signal, SignalClass, SignalStatus, SourceKind, TargetKey,
};
pub use verdict::Verdict;
