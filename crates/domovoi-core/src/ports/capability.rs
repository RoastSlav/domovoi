//! The read-only capability view the read and execution boundaries consult.
//!
//! The engine must ask *whether the hub would offer this here* without depending on the
//! hub that answers it, so this query API is a port: the hub's registry implements it, and
//! the testkit's fake does too. Nothing here evaluates policy, approves anything or
//! dispatches a run: "available" is not "approved", and the registry can never become a
//! second authorization decision.

use async_trait::async_trait;

use crate::capability::{Availability, CatalogSnapshot, ContractId, ContractIndex, EffectClass};
use crate::id::HostId;
use crate::skill::{SkillName, SkillSummary};

/// The one sentence the read boundary and the execution boundary refuse a disabled skill with.
///
/// Shared so the refusal a person sees does not depend on which boundary stopped the call.
pub const DISABLED_REASON: &str = "the skill is disabled on this host";

/// What a host's agent said about itself in wire 1.2's `hello`, in the terms the
/// capability registry reads.
///
/// A hub-side view, not the wire types: `domovoi-core` describes what a status means to
/// availability, and the hub converts its own message types into this.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostWords {
    /// `hello.runs_as_root`: a label about the agent, never a refusal by itself.
    pub runs_as_root: bool,
    /// `hello.in_container`: a label about the agent, never a refusal by itself.
    pub in_container: bool,
    /// What the isolated skill runner reported about itself.
    pub runner: RunnerState,
    /// What the privileged helper reported about itself.
    pub helper: HelperState,
    /// The operations the helper offers right now, as the agent reported them in `hello` and
    /// `agent_status` (group names and operation names such as `unit.restart`). Empty unless
    /// the helper is `Ready`.
    pub helper_ops: Vec<String>,
    /// `hello.ceiling_summary`, as the agent sent it.
    pub ceiling_summary: Option<String>,
    /// `hello.ceiling_hash`, as lowercase hex.
    ///
    /// Opaque: a caller compares it for change detection and never interprets it.
    pub ceiling_hash: Option<String>,
    /// Whether the agent reports a usable container runtime.
    pub container_runtime: bool,
}

/// One host's announced facts, all read from one authenticated session.
///
/// Words and skills are announced together in the handshake, so they are read together: a
/// reconnect between two separate reads would otherwise let a caller combine one session's
/// status with another session's skills.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostView {
    /// The 1.2 status words the session announced about itself.
    pub words: HostWords,
    /// The skills the session announced.
    pub skills: Vec<SkillSummary>,
}

/// How the agent's isolated skill runner reported itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunnerState {
    /// No runner is installed or configured.
    #[default]
    Absent,
    /// A runner exists but this host cannot use it.
    Unsupported,
    /// The runner is present and working.
    Ready,
    /// A runner exists but reported an error.
    Failed,
}

/// How the agent's privileged helper reported itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelperState {
    /// No helper is installed or configured.
    #[default]
    Absent,
    /// The helper is present and working.
    Ready,
    /// The helper exists but reported an error.
    Failed,
}

/// The hub's answer to "is this capability available here, and if not, why not".
///
/// Implemented by the hub's registry, which combines the compiled contracts, the agent's
/// implementation list, the host's own state, hub enablement, the raise-only floor and the
/// agent's ceiling summary. It consults no policy rule and cannot approve or refuse a run
/// beyond the states named in section 6.1.
#[async_trait]
pub trait CapabilityRegistry: Send + Sync {
    /// Whether `contract` is available on `host`, and why not.
    ///
    /// May walk the agent's catalogue on a miss, and never holds an internal lock across
    /// an `.await`.
    async fn availability(&self, host: HostId, contract: &ContractId) -> Availability;

    /// This host's catalogue snapshot, walked as far as it can be.
    async fn catalog(&self, host: HostId) -> CatalogSnapshot;

    /// The hub's compiled contract set: the only source of hub-known contract ids.
    fn contracts(&self) -> &ContractIndex;

    /// The effective effect class of `skill` on `host`, its declared class raised by any
    /// hub floor, or `None` when the host does not announce that skill.
    fn effective_effect(&self, host: HostId, skill: &SkillName) -> Option<EffectClass>;

    /// Whether the hub enables `contract` on `host`. In-memory, so the hot path never
    /// waits on it.
    fn enabled(&self, host: HostId, contract: &ContractId) -> bool;

    /// The skills the hub may offer on `host`: announced by its agent **and** enabled
    /// here. The agent's own list stays unfiltered on `AgentLink::skills`.
    fn enabled_skills(&self, host: HostId) -> Vec<SkillSummary>;
}
