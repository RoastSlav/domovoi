//! Reading a unit's identity and dispatching a restart through the host's agent.
//!
//! Like `PackageRelay`: `dispatch_restart` failing is proof nothing was handed over, and an
//! error from `restart_result` after the hand-off is never proof of either outcome.

use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;

use super::agent_link::LinkError;
use super::caps::{AvailabilityProbe, HostObservation, JournalStatus};
use crate::action::JournalEpoch;
use crate::id::{AgentSessionId, HostId};
use crate::install::CapabilityHandoff;
use crate::unit::{UnitDispatch, UnitName, UnitOutputs};

/// Reading unit identities and running `unit.restart`.
#[async_trait]
pub trait UnitRelay: Send + Sync {
    /// Reads the identity of `name` from `host`'s helper, in the live `session`, and waits for
    /// the answer until `deadline`.
    async fn read_identity(
        &self,
        host: HostId,
        name: &UnitName,
        session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<UnitOutputs, LinkError>;

    /// Hands one restart request to `host`'s live session and returns once the frame is queued.
    async fn dispatch_restart(
        &self,
        host: HostId,
        req: UnitDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError>;

    /// Waits for the terminal answer to a request handed over by
    /// [`UnitRelay::dispatch_restart`].
    async fn restart_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<UnitOutputs, LinkError>;
}

/// The capabilities unit restarts need: ask, read the session, ask the journal, relay.
#[derive(Clone)]
pub struct UnitCaps {
    /// Asking whether a host's agent is reachable.
    pub probe: Arc<dyn AvailabilityProbe>,
    /// Reading announced session facts, never a tool's output.
    pub observer: Arc<dyn HostObservation>,
    /// Reading the journal epoch and asking about executions.
    pub journal: Arc<dyn JournalStatus>,
    /// Reading identities and running the unit capabilities.
    pub relay: Arc<dyn UnitRelay>,
}
