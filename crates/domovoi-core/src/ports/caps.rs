//! Narrow capability objects for reaching agents (ADR 0023).
//!
//! The unrestricted link can perform every link operation at once. Engine subsystems
//! must not hold it: these objects name only the operations the subsystem in front of
//! them needs, and the hub composes them once, from the one link, at its runtime
//! boundary. A capability object cannot start a skill it cannot name, and cannot
//! read a host it cannot name, so the boundary is the type, not a scanner's list.
//!
//! [`ActionCaps`] and [`EvidenceCaps`] are the shapes the engine's dependency
//! structures carry; both are built around the hub's link and handed out separately.

use async_trait::async_trait;
use std::sync::Arc;

use jiff::Timestamp;

use super::agent_link::{
    ActionAnswers, AgentSnapshot, CheckRecheck, HostAvailability, LinkError, Presence,
    SkillDispatch, SkillHandoff, SkillOutput, ToolOutput,
};
use crate::action::JournalEpoch;
use crate::id::{AgentSessionId, ExecutionId, HostId};
use crate::install::{
    CapabilityHandoff, PackageDispatch, PackageOutputs, TransferAnswer, TransferFrame,
};
use crate::signal::{CheckKey, TargetKey};
use crate::skill::SkillSummary;
use crate::triage::ReadToolCall;

/// Asking whether a host's agent can be reached now.
///
/// The answer distinguishes a host with no agent connected from one whose agent left
/// or was rejected, which the raw connection state alone cannot tell apart.
#[async_trait]
pub trait AvailabilityProbe: Send + Sync {
    /// Whether `host`'s agent is reachable now.
    async fn check_availability(&self, host: HostId) -> HostAvailability;
}

/// Reading what a live session announced, never running anything.
pub trait HostObservation: Send + Sync {
    /// The coherent session snapshot for `host`, or `None` with no live session.
    fn observe_snapshot(&self, host: HostId) -> Option<AgentSnapshot>;

    /// Skills the host's agent announced on its last handshake; empty when never
    /// connected.
    fn observed_skills(&self, host: HostId) -> Vec<SkillSummary>;

    /// The current connection state of `host`'s agent.
    fn observe_presence(&self, host: HostId) -> Presence;
}

/// Reading one tool's output from a host.
///
/// This is the only capability that lets a caller see a host's tool output; it
/// reaches one read tool call.
///
/// Deadline contract: every await inside `read_host`, the error classification and any
/// fallback lookup included, ends by `deadline` with [`LinkError::Timeout`], and a request
/// the implementation registered is released however the call ends, a dropped future
/// included. `Evidence` races only the availability check itself; this contract is what
/// bounds the read.
#[async_trait]
pub trait HostRead: Send + Sync {
    /// Runs one read tool on `host` and answers with its output.
    async fn read_host(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError>;
}

/// Handing one approved skill to a host and waiting for its terminal answer.
///
/// `hand_off` failing is proof nothing was handed over; an error from
/// `await_handoff_result` after the hand-off is never proof of either outcome
/// (ADR 0022).
#[async_trait]
pub trait HandoffRunner: Send + Sync {
    /// Hands one approved skill to `host`'s live session, which must still be the
    /// exact `expected_session` and the journal `expected_epoch` names, and returns
    /// once the frame is in that session's outgoing queue.
    async fn hand_off(
        &self,
        host: HostId,
        req: SkillDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<SkillHandoff, LinkError>;

    /// Waits for the terminal answer to a skill request handed over by
    /// [`HandoffRunner::hand_off`].
    async fn await_handoff_result(
        &self,
        host: HostId,
        handoff: SkillHandoff,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError>;
}

/// Reading the journal epoch and asking what the agent knows about actions.
#[async_trait]
pub trait JournalStatus: Send + Sync {
    /// The journal epoch the host's live session reported, or `None` with no live
    /// session.
    fn epoch_of(&self, host: HostId) -> Option<JournalEpoch>;

    /// What the host's agent knows about `ids` under `expected_epoch`. At most 64
    /// ids.
    async fn status_batch(
        &self,
        host: HostId,
        ids: Vec<ExecutionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError>;
}

/// Re-running one check on a host, for verification after an action.
#[async_trait]
pub trait CheckRerun: Send + Sync {
    /// Re-runs one check on `host`.
    async fn rerun_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError>;
}

/// Relaying package files to a host and running its package capabilities.
///
/// This is the only object that can put bytes on an agent or start `agent.package.*`.
/// `dispatch_package` failing is proof nothing was handed over; an error from
/// `package_result` after the hand-off is never proof of either outcome.
#[async_trait]
pub trait PackageRelay: Send + Sync {
    /// Sends one frame of a transfer to `host`'s live session, which must still be
    /// `expected_session`. Only an `End` frame is answered, with the agent's verdict.
    async fn relay_transfer(
        &self,
        host: HostId,
        frame: TransferFrame,
        expected_session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<Option<TransferAnswer>, LinkError>;

    /// Hands one package request to `host`'s live session and returns once the frame is in
    /// that session's outgoing queue.
    async fn dispatch_package(
        &self,
        host: HostId,
        req: PackageDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError>;

    /// Waits for the terminal answer to a request handed over by
    /// [`PackageRelay::dispatch_package`].
    async fn package_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<PackageOutputs, LinkError>;
}

/// The capabilities the action services need, as separate objects.
#[derive(Clone)]
pub struct ActionCaps {
    /// Asking whether a host's agent is reachable.
    pub probe: Arc<dyn AvailabilityProbe>,
    /// Reading announced session facts, never a tool's output.
    pub observer: Arc<dyn HostObservation>,
    /// Reading the journal epoch and asking about action states.
    pub journal: Arc<dyn JournalStatus>,
    /// Handing an approved skill over and waiting for its answer.
    pub handoff: Arc<dyn HandoffRunner>,
    /// Re-running a check to verify an action.
    pub rerun: Arc<dyn CheckRerun>,
}

/// The capabilities evidence collection needs: ask, read the catalogue, read once.
#[derive(Clone)]
pub struct EvidenceCaps {
    /// Asking whether a host's agent is reachable, before any read.
    pub probe: Arc<dyn AvailabilityProbe>,
    /// The catalogue view used to validate a read skill before running it.
    pub observer: Arc<dyn HostObservation>,
    /// The one capability that can see a host's tool output.
    pub read: Arc<dyn HostRead>,
}

/// The capabilities package installation needs: ask, read the session, ask the journal, relay.
#[derive(Clone)]
pub struct PackageCaps {
    /// Asking whether a host's agent is reachable.
    pub probe: Arc<dyn AvailabilityProbe>,
    /// Reading announced session facts, never a tool's output.
    pub observer: Arc<dyn HostObservation>,
    /// Reading the journal epoch and asking about executions.
    pub journal: Arc<dyn JournalStatus>,
    /// Relaying files and running the package capabilities.
    pub relay: Arc<dyn PackageRelay>,
}
