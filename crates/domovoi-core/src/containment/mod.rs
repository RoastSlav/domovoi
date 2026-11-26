//! Containment: a timed inbound block of one external address or small prefix.
//!
//! Everything here is pure. The root helper renders the scripts and applies the exclusion
//! algorithm; the hub uses the same functions to compute what a block would overlap, so the two
//! cannot drift. See ADR 0038.

mod dispatch;
mod exclusion;
mod gate;
mod ip;
mod journal;
pub mod keys;
mod net;
pub mod ops;
pub mod templates;
mod tokens;

pub use dispatch::{BlockSpec, ContainCall, ContainDispatch, ContainOutputs, DispatchError};
pub use exclusion::{
    ExclusionInputs, ExclusionRefusal, TargetPolicy, TargetRefusal, check_target, coalesce,
    effective_exclusions, fixed_exclusions,
};
pub use gate::{DrillsState, Gate, SelftestKey, SelftestResult, UntestedReason, Verdict, gate_for};
pub use journal::{
    JournalCorrupt, JournalOp, JournalRead, JournalRecord, RecreateReason, read_journal,
};
pub use net::{Family, Net, NetError, addr_text};
pub use templates::Layout;
pub use tokens::{
    AbsentClass, AbsentFacts, NftOp, StderrClass, Token, classify_absent, classify_stderr,
    format_message, parse_message,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_dispatch;
#[cfg(test)]
mod tests_templates;
