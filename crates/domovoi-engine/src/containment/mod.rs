//! The containment driver: runs the three phases of one block (`apply`, `confirm`, `rollback`)
//! of `firewall.block_source`, settles it in `containment_blocks`, and reconciles it from the
//! agent's journal.
//!
//! One approval covers exactly one block: all three phases carry the same plan hash and the same
//! authorization, each under its own execution id, and name the block by the apply's execution
//! id. The hub alone sends `confirm`, after the apply answered, because the confirmation
//! reaching the agent proves the path in both directions. Nothing is retried after a hand-off
//! and nothing is ever re-applied automatically (ADR 0038).

use std::sync::Arc;

use domovoi_core::ports::{
    Clock, ContainCaps, ContainmentRepo, HelperTrustRead, TargetResolver, UnitOfWork,
};
use jiff::SignedDuration;

pub use domovoi_core::containment::{
    BlockSpec, ContainCall, ContainDispatch, ContainOutputs, DispatchError,
};

pub(crate) mod audit;
mod driver;
pub mod input;
pub mod permit;
pub(crate) mod phases;
mod reconcile;
mod run;
mod status;

pub use driver::{ContainError, ContainmentDriver};
pub use input::{Assessment, PathSource};
pub use phases::{AuthorizedContainment, ContainEnd, Prepared};
pub use status::{HostContainment, ObservedBlock};

/// The longest the hub may take, after the apply answered, to send `confirm`. The kernel's 30
/// second timeout runs from the helper's `create`, so the budget leaves margin for the round
/// trip.
pub const CONFIRM_WITHIN: SignedDuration = SignedDuration::from_secs(20);

/// The kernel timeout of step one, in seconds. A block never confirmed is gone after this long.
pub const STEP_ONE_SECS: i64 = 30;

/// The shortest authorization window a block may start with: apply, confirm and a rollback
/// all need to fit inside it.
pub const MIN_WINDOW: SignedDuration = SignedDuration::from_secs(90);

/// What the contract allows one phase to take before the grace is added.
pub const PHASE_TIMEOUT: SignedDuration = SignedDuration::from_secs(60);

/// What the driver is built from: ports only. The agent link is reached through the relay.
#[derive(Clone)]
pub struct ContainDeps {
    /// Availability, the session snapshot, the journal and the relay.
    pub caps: ContainCaps,
    /// One atomic commit for a phase change, its journal row and its audit row.
    pub uow: Arc<dyn UnitOfWork>,
    /// The stored blocks, read only.
    pub blocks: Arc<dyn ContainmentRepo>,
    /// What the hub holds about each host's helper.
    pub trust: Arc<dyn HelperTrustRead>,
    /// The discovered management paths a verified report must cover.
    pub paths: Arc<dyn PathSource>,
    /// The canonical identity, enrolled key included, a plan binds.
    pub resolver: Arc<dyn TargetResolver>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Extra time beyond a contract's timeout to wait for an agent's answer.
    pub result_grace: SignedDuration,
}

#[cfg(test)]
mod tests;
