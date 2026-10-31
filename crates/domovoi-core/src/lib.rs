//! Domain types, state machines and ports (traits) for Domovoi.
//!
//! This crate is pure: no I/O, no async runtime, no adapters. Everything else
//! in the workspace depends on it. See `ARCHITECTURE.md` §5 (domain model) and
//! §6 (ports).

pub mod ack;
pub mod action;
pub mod advisory;
pub mod baseline;
pub mod card;
pub mod digest;
pub mod error;
pub mod fingerprint;
pub mod history;
pub mod host;
pub mod id;
pub mod incident;
pub mod interaction;
pub mod note;
pub mod ports;
pub mod severity;
mod sha256;
pub mod signal;
pub mod skill;
pub mod triage;
pub mod user;
pub mod verdict;

pub use ack::{AckRule, EvidencePredicate, MaintenanceWindow, TargetPattern};
pub use action::{
    ActionChange, ActionError, ActionRequest, ActionStatus, ApprovalChannel, HumanApproval,
    PlanHash, Principal, StoredAction,
};
pub use advisory::{Advisory, AdvisoryKind};
pub use baseline::{BaselineEntry, BaselineKind, Criticality};
pub use card::{Card, CardBadge, CardButton};
pub use digest::{Digest, DigestFact, FactValue, HostDigest, Trend};
pub use error::{TransitionError, ValidationError};
pub use fingerprint::Fingerprint;
pub use history::EntityHistory;
pub use host::{AgentIdentity, Host, HostKind, HostName};
pub use id::{
    AckRuleId, ActionId, AdvisoryId, AgentId, HostId, IncidentId, NoteId, RequestId, SignalId,
    UserId,
};
pub use incident::{
    Incident, IncidentChange, IncidentEvent, IncidentPolicy, IncidentStatus, apply,
};
pub use interaction::{Interaction, InteractionKind, InteractionReply};
pub use note::Note;
pub use severity::Severity;
pub use signal::{
    CheckKey, Evidence, EvidenceValue, Signal, SignalClass, SignalStatus, SourceKind, TargetKey,
};
pub use skill::{
    ArgError, ManifestError, ManifestHash, ParamKind, ParamName, ParamSpec, RevertSpec, Risk,
    SkillManifest, SkillName, SkillSummary, ValidatedArgs, VerifySpec,
};
pub use triage::{
    ActionProposal, EvidenceRef, ReadTool, ReadToolCall, ToolArgError, TriageBudget, TriageOutcome,
    TriageReport,
};
pub use user::{Session, User};
pub use verdict::Verdict;
