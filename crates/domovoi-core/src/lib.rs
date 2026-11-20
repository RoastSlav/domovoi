//! Domain types, state machines and ports (traits) for Domovoi.
//!
//! This crate is pure: no I/O, no async runtime, no adapters. Everything else
//! in the workspace depends on it. See `ARCHITECTURE.md` §5 (domain model) and
//! §6 (ports).

pub mod ack;
pub mod action;
pub mod actor;
pub mod addr;
pub mod advisory;
pub mod audit_chain;
pub mod baseline;
pub mod capability;
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
pub mod notify;
pub mod operation;
pub mod package;
pub mod policy_store;
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
    ActionChange, ActionError, ActionRequest, ActionStatus, ApprovalChannel, ApprovalEvidenceError,
    ApprovalPlan, ApprovalProvenance, Authorization, AuthorizationKind, HumanApproval, ImplHash,
    JournalEpoch, PermitError, PlanBinding, PlanContract, PlanError, PlanFamily, PlanHash,
    PlanTarget, PolicyPermit, PolicyPermitParts, Principal, StoredAction, StoredActionError,
};
pub use actor::{
    Actor, ActorParseError, SYSTEM_ACTIONS, SYSTEM_AUDIT, SYSTEM_INCIDENTS, SYSTEM_OPERATIONS,
};
pub use addr::{in_cidr, valid_cidr};
pub use advisory::{Advisory, AdvisoryKind};
pub use baseline::{BaselineEntry, BaselineKind, Criticality};
pub use capability::{
    Availability, AvailabilityEntry, CapabilityContract, CatalogSnapshot, ContractId, EffectClass,
    ResolvedTarget, TargetError, TargetKind, TargetQuery,
};
pub use card::{Card, CardBadge, CardButton, CardPlan};
pub use digest::{Digest, DigestFact, FactValue, HostDigest, Trend};
pub use error::{TransitionError, ValidationError};
pub use fingerprint::Fingerprint;
pub use history::EntityHistory;
pub use host::{
    AgentIdentity, AgentReport, AuthenticatedReport, Host, HostKind, HostName, KeyFingerprint,
    ProtocolParseError, REPORT_REFRESH, ReportOutcome, ReportedProtocol, host_target_text,
};
pub use id::{
    AckRuleId, ActionId, AdvisoryId, AgentId, AgentSessionId, EffectId, ExecutionId, HostId,
    IncidentId, NoteId, OperationId, PolicyVersionId, RequestId, SignalId, StepId, UserId,
};
pub use incident::{
    Incident, IncidentChange, IncidentEvent, IncidentGroup, IncidentPolicy, IncidentStatus, apply,
};
pub use interaction::{Interaction, InteractionKind, InteractionReply};
pub use note::Note;
pub use notify::{ChatRefs, HeldItem, NotifyState};
pub use operation::{
    BudgetKind, BudgetLimits, BudgetUsed, Budgets, EdgeError, EdgeKind, ExecutionApproval,
    ExecutionError, ExecutionIntent, ExecutionParts, ExecutionRecord, ExecutionState,
    FinishOutcome, Operation, OperationKind, OperationSnapshot, OperationStatus, OperationTrace,
    PauseReason, PinnedSubject, RetryOf, Step, StepEdge, StepIntent, StepKind, StepOutcome,
    StepProgress, StepRecord, StepUsage, TRACE_SCHEMA_VERSION, TraceEdge, TraceOperation, TraceRef,
    TraceStep, WaitKind, build_trace, check_edge,
};
pub use severity::Severity;
pub use sha256::ScriptDigest;
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
