//! Self-test model: check ids, the report, the isolation evaluator and platform tiers.
//!
//! Pure. Collection happens in `domovoi-helper` and the agent; every decision is made here so it
//! can be tested with fakes and shared between the installer, the helper and the hub.

mod boundary;
mod check;
mod evaluate;
mod probe;
mod report;
#[cfg(test)]
mod tests;
mod tier;

pub use boundary::{BoundaryReason, BoundaryState, evaluate_boundary, inspect_boundary};
pub use check::{CheckId, CheckResult, Outcome, RunBy};
pub use evaluate::{
    IsolationFault, RunnerFault, evaluate_isolation, evaluate_runner_startup, parse_proc_status,
};
pub use probe::{
    AccessRecord, IncomingFacts, IsolationReport, MountFact, MountRole, ProcStatus, RunFacts,
};
pub use report::SelftestReport;
pub use tier::{
    PLATFORM_RECORD_FORMAT, PlatformId, PlatformRecord, PlatformRecordState, PlatformTier,
    REQUIRED_CHECKS, ReadDecision, SUPPORTED_PLATFORMS, SkillsDecision, TierDecision, TierNotice,
    WriteDecision, WriteOptIn, classify_platform,
};
