//! Application services for Domovoi.
//!
//! Correlation, the single-writer incident manager, notification policy,
//! triage orchestration, approvals and digests. Depends only on
//! `domovoi-core` ports; never performs I/O directly. See `ARCHITECTURE.md` §7
//! and ADR 0011.
//!
//! - [`correlate`] picks the incident a signal belongs to; [`suppress`] decides whether a
//!   change is notified. Both are pure.
//! - [`manager`] is the incident manager: [`manager::decide`] is the pure decision
//!   function, [`manager::IncidentManager`] the task that stores its writes and then
//!   publishes [`events::DomainEvent`]s.
//! - [`notify`] decides what to say about each event and when, [`cards`] builds the card,
//!   [`digest`] the daily digest, [`advisories`] the hygiene findings, and [`render`] turns
//!   them into short markdown. All pure; sending happens elsewhere.
//! - [`operations`] approves steps (only with a human approval), re-validates, dispatches,
//!   verifies and audits them.
//! - [`triage`] runs the model over read-only tools and attaches its report; [`redact`]
//!   removes secrets from everything that reaches a prompt or leaves the hub.

pub mod advisories;
pub mod cards;
pub mod chat_token;
pub mod containment;
pub mod context;
pub mod conversations;
pub mod correlate;
pub mod digest;
pub mod effects;
pub mod events;
pub mod evidence;
pub mod knowledge;
pub mod manager;
pub mod notify;
pub mod operations;
pub mod packages;
pub mod plan;
pub mod policy;
pub mod policy_shadow;
pub mod redact;
pub mod render;
pub mod suppress;
pub mod targets;
pub mod telemetry;
pub mod triage;

pub use effects::{EffectHandler, EffectOutcome, EffectRunner, EffectRunnerDeps, EffectSettings};
pub use events::{DomainEvent, EventKind};
pub use evidence::{Evidence, EvidenceError, EvidenceRequest, EvidenceScope, HostCatalog};
pub use manager::{Command, CommandError, IncidentManager, ManagerDeps, ManagerHandle};
pub use policy::EnginePolicy;
pub use redact::PatternRedactor;
pub use triage::{TriageDeps, TriageError, TriageHandle, TriageService, TriageSettings};
