//! The one path to an agent's tool output.
//!
//! [`Evidence::collect_evidence`] is the only caller of
//! [`domovoi_core::ports::AgentLink::run_tool`]: the model, the
//! Operator, the triage context and the hub's profile collection all read a host through it,
//! and `xtask arch-check` (EXEC-1) fails when another module reads a host directly.
//! The request deadline bounds the whole operation: an availability check that stays
//! pending past the deadline answers as timed out, and the read never runs after it.
//!
//! The boundary decides *whether* a read may happen (the host the request was built for,
//! the host's availability, then the read's enablement) and hands back the agent's
//! [`ToolOutput`] unchanged. Anything that shapes what is shown - redaction, truncation,
//! the "[the agent cut this output]" note - stays with the caller that owns it, so moving
//! a read here never changes the text a person or the model sees.
//!
//! It is not the only way to obtain information about a host: the hub reads host metadata
//! through `AgentAdmin`, and verification re-runs a check with
//! [`domovoi_core::ports::AgentLink::run_check`]. [`HostCatalog`] is
//! the narrow, non-tool view of an agent (its session, announced skills and presence) that
//! triage and profile collection need beside the reads.

use std::sync::Arc;

use domovoi_core::ports::{
    AgentSnapshot, AvailabilityProbe, Clock, EvidenceCaps, HostAvailability, HostObservation,
    HostRead, LinkError, Presence, ToolOutput,
};
use domovoi_core::{HostId, Incident, ReadToolCall, Risk, SkillSummary};
use jiff::Timestamp;

/// Which host a caller may read.
///
/// The scope is built from the thing the caller works on, so the host that reaches the agent
/// is the host the caller was given and never a second argument beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceScope {
    /// The host of an incident the caller is working on.
    Incident(HostId),
    /// A host the caller was given directly.
    Host(HostId),
}

impl EvidenceScope {
    /// The host this scope allows.
    #[must_use]
    pub fn host(self) -> HostId {
        match self {
            Self::Incident(host) | Self::Host(host) => host,
        }
    }
}

/// One read a caller wants, with the host it may touch and the deadline it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRequest {
    scope: EvidenceScope,
    call: ReadToolCall,
    deadline: Timestamp,
}

impl EvidenceRequest {
    /// A read on the host of `incident`.
    #[must_use]
    pub fn for_incident(incident: &Incident, call: ReadToolCall, deadline: Timestamp) -> Self {
        Self {
            scope: EvidenceScope::Incident(incident.host()),
            call,
            deadline,
        }
    }

    /// A read on a host the caller was given.
    #[must_use]
    pub fn for_host(host: HostId, call: ReadToolCall, deadline: Timestamp) -> Self {
        Self {
            scope: EvidenceScope::Host(host),
            call,
            deadline,
        }
    }

    /// The host the read will touch; the only host that reaches the agent.
    #[must_use]
    pub fn host(&self) -> HostId {
        self.scope.host()
    }

    /// The scope the request was built from.
    #[must_use]
    pub fn scope(&self) -> EvidenceScope {
        self.scope
    }

    /// The read tool that will run.
    #[must_use]
    pub fn call(&self) -> &ReadToolCall {
        &self.call
    }
}

/// Why a read was refused or failed.
///
/// The connected, incompatible and timeout cases carry the same text the link gives, and a
/// link failure keeps the link's own message, so a tool result reads exactly as it did
/// before the read moved behind this boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceError {
    /// The host has no connected agent.
    #[error("agent not connected")]
    NotConnected,
    /// The host's agent speaks another protocol version.
    #[error("agent_incompatible: the agent speaks another protocol version; upgrade it")]
    Incompatible,
    /// No answer before the deadline.
    #[error("timed out")]
    Timeout,
    /// The link failed for another reason; the text is the link's own.
    #[error("{0}")]
    Transport(String),
    /// The read tool or read skill is not enabled on this host; the text says which.
    #[error("{0}")]
    NotEnabled(String),
}

impl From<LinkError> for EvidenceError {
    fn from(err: LinkError) -> Self {
        match err {
            LinkError::NotConnected => Self::NotConnected,
            LinkError::Incompatible => Self::Incompatible,
            LinkError::Timeout => Self::Timeout,
            other => Self::Transport(other.to_string()),
        }
    }
}

/// The one read-capable handle to the agents; cheap to clone.
#[derive(Clone)]
pub struct Evidence {
    /// Asks whether a host is reachable, before any read.
    probe: Arc<dyn AvailabilityProbe>,
    /// The catalogue view used to validate a read skill before running it.
    observer: Arc<dyn HostObservation>,
    /// The one capability that can see a host's tool output.
    read: Arc<dyn HostRead>,
    /// The time the request deadline is measured against.
    clock: Arc<dyn Clock>,
}

impl Evidence {
    /// Wraps the capabilities the hub composes for the engine, and the clock the
    /// deadline is measured against.
    #[must_use]
    pub fn new(caps: EvidenceCaps, clock: Arc<dyn Clock>) -> Self {
        Self {
            probe: caps.probe,
            observer: caps.observer,
            read: caps.read,
            clock,
        }
    }

    /// Checks that the host is reachable, then that the read is enabled, then reads once.
    ///
    /// The request's deadline bounds the whole operation. A passed deadline answers as
    /// timed out before anything is asked, and an availability check that stays pending
    /// past it does the same. After that the read itself is bounded by the
    /// [`HostRead`] deadline contract, which the hub's `WsAgentLink` meets for the
    /// fallback lookups of a vanished session as well as for the network wait; this method
    /// does not wrap the read in a second race, because dropping it mid-flight is exactly
    /// what the adapter's own bounds avoid. Availability comes first, so a host that is unknown, absent or
    /// speaks another protocol version always answers as such; a skill that is missing, not
    /// read-only or called with arguments its manifest rejects can only be reported for a
    /// host that is connected now. Enablement at M1 is structural: [`ReadToolCall`] is a
    /// closed enum, so the tools that exist are the tools that may run, and the one extra
    /// rule is that a read skill must be installed, read-only and given arguments its
    /// manifest accepts. The agent's output is returned verbatim.
    pub async fn collect_evidence(
        &self,
        request: EvidenceRequest,
    ) -> Result<ToolOutput, EvidenceError> {
        let host = request.host();
        if self.clock.now() >= request.deadline {
            return Err(EvidenceError::Timeout);
        }
        // Availability is awaited under the same deadline: a probe that stays pending
        // past it is answered as a timeout, and a host that is unknown, absent or
        // incompatible never reaches the read.
        let availability = tokio::select! {
            found = self.probe.check_availability(host) => found,
            () = self.clock.sleep_until(request.deadline) => {
                return Err(EvidenceError::Timeout);
            }
        };
        match availability {
            HostAvailability::Known { .. } => {}
            HostAvailability::Unknown | HostAvailability::Absent { .. } => {
                return Err(EvidenceError::NotConnected);
            }
            HostAvailability::Incompatible { .. } => return Err(EvidenceError::Incompatible),
        }
        if self.clock.now() >= request.deadline {
            return Err(EvidenceError::Timeout);
        }
        if let ReadToolCall::RunReadSkill { skill, args } = &request.call {
            self.read_skill_is_enabled(host, skill, args)?;
        }
        self.read
            .read_host(host, request.call, request.deadline)
            .await
            .map_err(EvidenceError::from)
    }

    /// The one enablement rule that exists before the capability registry: a read skill must
    /// be on the host, declared read-only, and accept the arguments it was called with.
    fn read_skill_is_enabled(
        &self,
        host: HostId,
        skill: &domovoi_core::SkillName,
        args: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), EvidenceError> {
        let summary = self
            .observer
            .observed_skills(host)
            .into_iter()
            .find(|candidate| &candidate.name == skill)
            .ok_or_else(|| {
                EvidenceError::NotEnabled(format!("skill `{skill}` is not installed on this host"))
            })?;
        if summary.risk != Risk::Read {
            return Err(EvidenceError::NotEnabled(format!(
                "skill `{skill}` is not read-only"
            )));
        }
        summary
            .validate_args(args)
            .map(|_| ())
            .map_err(|err| EvidenceError::NotEnabled(err.to_string()))
    }
}

/// The narrow view of a host's agent that a caller needs beside its reads: the session the
/// plan is pinned to, the announced skill catalogue and the connection state.
///
/// It never runs a tool; using it cannot read a host's output.
#[derive(Clone)]
pub struct HostCatalog {
    observer: Arc<dyn HostObservation>,
}

impl HostCatalog {
    /// Wraps the same catalogue capability [`Evidence`] was built from.
    #[must_use]
    pub fn new(observer: Arc<dyn HostObservation>) -> Self {
        Self { observer }
    }

    /// The coherent session snapshot for `host`, or `None` with no live session.
    #[must_use]
    pub fn snapshot(&self, host: HostId) -> Option<AgentSnapshot> {
        self.observer.observe_snapshot(host)
    }

    /// The skills the host's agent announced on its last handshake.
    #[must_use]
    pub fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.observer.observed_skills(host)
    }

    /// Current connection state of the host's agent.
    #[must_use]
    pub fn presence(&self, host: HostId) -> Presence {
        self.observer.observe_presence(host)
    }
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
