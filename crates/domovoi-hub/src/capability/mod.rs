//! The hub's capability registry: which contracts each host can run, and why.
//!
//! The hub, not the agent, decides availability: it owns the contract index, the per-host
//! enablement and skill floors, and the agent's reported status (ADR 0018, 0023). The agent
//! only reports what it has. These modules combine the compiled contract index, the agent's own
//! words (the catalogue walk, `hello.skills`, `agent_status`) and the owner's settings keys
//! into one read-only view, the `CapabilityRegistry` port the engine and the API consume.
//!
//! The registry never evaluates policy, never approves and never dispatches: it answers
//! "is this contract here, and enabled" only (RW-10). Enforcing enablement at the final
//! boundary stays with the engine (`collect_evidence`, `execute`), which asks
//! [`Enablement::is_disabled`] through the registry.

mod catalogue;
mod ceiling;
mod enablement;
mod floors;
mod host;
mod host_rules;
mod inventory;
mod registry;

pub use catalogue::CatalogueCache;
pub use ceiling::{
    CeilingParse, CeilingReport, CeilingState, covers, parse_ceiling, report_ceiling,
};
pub use enablement::{DISABLED_REASON, Enablement, host_skills_key};
pub use floors::{FloorWriteError, Floors, host_floors_key};
pub use host::HostCapabilities;
pub use inventory::{ContainerRef, InventoryError, parse_inspect_id, parse_ps};
pub use registry::{HubRegistry, RegistryDeps, RegistryError};
