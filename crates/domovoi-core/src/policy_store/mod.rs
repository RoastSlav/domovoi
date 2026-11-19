//! The data model of stored policy: versions, their rules, acknowledgements and recorded
//! shadow decisions. Plain serde data with no I/O and no knowledge of the evaluator: rule
//! bodies and decision documents are opaque text here, and their formats belong to the engine.

mod shadow;
mod version;
mod write;

pub use shadow::{
    ShadowCandidate, ShadowFilter, ShadowRecord, ShadowState, ShadowStatus, ShadowSummary,
    SweepStats,
};
pub use version::{AckRecord, PolicyStatus, PolicyVersionRecord, StoredRule};
pub use write::{DraftGuard, PolicyWrite};

/// The most rules one policy version may hold.
pub const MAX_RULES: usize = 500;
/// The longest note a rule may carry, in characters.
pub const MAX_NOTE_CHARS: usize = 500;
/// The largest single JSON column of a [`ShadowRecord`], in bytes.
pub const MAX_ROW_JSON_BYTES: usize = 65_536;
