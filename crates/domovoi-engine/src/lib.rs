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

pub mod advisories;
pub mod cards;
pub mod correlate;
pub mod digest;
pub mod events;
pub mod manager;
pub mod notify;
pub mod policy;
pub mod render;
pub mod suppress;

pub use events::{DomainEvent, EventKind};
pub use manager::{Command, CommandError, IncidentManager, ManagerDeps, ManagerHandle};
pub use policy::EnginePolicy;
