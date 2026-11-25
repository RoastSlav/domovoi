//! Records, for every Execution step, what the active policy and its shadow rules would
//! decide, with the exact inputs used, so the evaluation can be replayed later.
//!
//! Nothing here enforces anything: no approval, dispatch or permit reads a recorded
//! decision. Inputs that do not exist yet (ceiling, helper attestation, fact producers,
//! autonomy state) are replaced by the fail-closed value the evaluator expects and listed
//! as gaps on the row, never invented as safe.
//!
//! This module is deliberately named apart from [`crate::policy`], so that module's own
//! guarantee (no approval, permit or authorization type appears under it) stays checkable.

mod codec;
mod doc;
mod env;
mod inputs;
mod recorded;
mod recorder;
mod replay;
mod report;
mod report_keys;

pub use codec::{CodecError, decode_rule, decode_rules, encode_rule, rules_hash, stored_rules};
pub use doc::{ConstraintsDoc, DecisionDoc, DecisionEnvelope, FactUse, error_code, outcome_text};
pub use env::{
    AutonomyFacts, AutonomyReading, CeilingReading, ContainmentReading, HostTarget,
    ShadowEnvironment,
};
pub use inputs::{Built, Verdict, build};
pub use recorded::{
    CandidateDoc, ConstrainableArgDoc, ContractDoc, GapInput, GapReason, InputGap, InputsEnvelope,
    PinnedDoc, RecordedInputs,
};
pub use recorder::{
    BATCH, LATE_AFTER, MAX_BATCHES_PER_SWEEP, SWEEP_EVERY, ShadowError, ShadowRecorder, SweepReport,
};
pub use replay::{
    Comparison, ReplayError, Replayed, compare_with_record, decode_decision, decode_inputs, replay,
    request_from_inputs,
};
pub use report::{SimulationSummary, summarise};
pub use report_keys::{LockChange, LockChangeKind, finding_key, locked_changes, preview_hash};
