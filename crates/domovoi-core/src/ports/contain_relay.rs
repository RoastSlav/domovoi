//! Dispatching the phases of a containment block through the host's agent.
//!
//! Like `UnitRelay`: `dispatch_phase` failing is proof nothing was handed over, and an error from
//! `phase_result` after the hand-off is never proof of either outcome. The one engine caller is
//! `domovoi_engine::containment::phases` (ADR 0038).

use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;

use super::agent_link::LinkError;
use super::caps::{AvailabilityProbe, HostObservation, JournalStatus};
use crate::action::JournalEpoch;
use crate::containment::{ContainDispatch, ContainOutputs};
use crate::id::{AgentSessionId, HostId};
use crate::install::CapabilityHandoff;

/// Running the phases of `firewall.block_source` and reading `firewall.list_blocks`.
#[async_trait]
pub trait ContainRelay: Send + Sync {
    /// Hands one request to `host`'s live session and returns once the frame is queued.
    async fn dispatch_phase(
        &self,
        host: HostId,
        req: ContainDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError>;

    /// Waits for the terminal answer to a request handed over by [`ContainRelay::dispatch_phase`].
    /// [`LinkError::Rejected`] carries the agent's confirmed refusal or failure, as
    /// `"<token>: <detail>"`; every other error leaves the outcome unknown.
    async fn phase_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<ContainOutputs, LinkError>;
}

/// The capabilities containment needs: ask, read the session, ask the journal, relay.
#[derive(Clone)]
pub struct ContainCaps {
    /// Asking whether a host's agent is reachable.
    pub probe: Arc<dyn AvailabilityProbe>,
    /// Reading announced session facts, never a tool's output.
    pub observer: Arc<dyn HostObservation>,
    /// Reading the journal epoch and asking about executions.
    pub journal: Arc<dyn JournalStatus>,
    /// Running the containment phases.
    pub relay: Arc<dyn ContainRelay>,
}
