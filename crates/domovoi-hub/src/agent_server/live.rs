//! The data one live agent session carries, and what the hub remembers about a host.

use std::collections::HashMap;
use std::net::IpAddr;

use domovoi_core::{AgentId, AgentSessionId, HostId, RequestId, SkillSummary};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    AgentStatus, CheckInfo, DockerReport, HelperStatus, HostFacts, SessionId, SkillRunnerStatus,
};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The liveness signal raised for a silent host, until it is heard from again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Liveness {
    /// The host answers on the network but its agent does not.
    AgentDown,
    /// Neither the agent nor the host answers.
    HostDown,
}

/// A live, authenticated session.
pub(crate) struct Live {
    pub session: SessionId,
    /// The hub-local identity of this authenticated connection.
    pub identity: AgentSessionId,
    /// The enrolled agent that authenticated it.
    pub agent: AgentId,
    pub since: Timestamp,
    pub version: String,
    /// What the agent said about its machine, kept so the hub can profile the host.
    pub facts: HostFacts,
    pub skills: Vec<SkillSummary>,
    /// The catalogue hash the agent announced in its handshake, as lowercase hex.
    pub catalog_hash: Option<String>,
    /// `hello.runs_as_root`: a label about the agent, never a refusal by itself.
    pub runs_as_root: bool,
    /// `hello.in_container`, refreshed by each `agent_status`.
    pub in_container: bool,
    /// What the agent's isolated skill runner reported about itself.
    pub skill_runner_status: SkillRunnerStatus,
    /// What the agent's privileged helper reported about itself.
    pub helper_status: HelperStatus,
    /// `hello.helper_identity`: the public key the agent relayed for its helper. Never
    /// trusted; the hub only compares it with the owner's pin.
    pub helper_identity: Option<[u8; 32]>,
    /// `hello.ceiling_summary`, kept as the agent sent it.
    pub ceiling_summary: Option<String>,
    /// `hello.ceiling_hash`, as lowercase hex. Opaque: compared, never interpreted.
    pub ceiling_hash: Option<String>,
    pub checks: Vec<CheckInfo>,
    pub docker: Option<DockerReport>,
    /// The action journal's epoch the agent announced.
    pub journal_epoch: Uuid,
    /// Whether this agent promised that every action execution is in its journal, so a
    /// missing record proves the execution never arrived. Older agents and agents with an
    /// unjournaled execution path answer no, and their absence is then not proof.
    pub authoritative_absence: bool,
    /// The last `agent_status` the agent sent, if any.
    pub agent_status: Option<AgentStatus>,
    /// Why the agent refused the hub's last check overrides; empty when it took them.
    pub override_refusals: Vec<String>,
    pub outbox: mpsc::Sender<String>,
    pub pending: HashMap<RequestId, oneshot::Sender<Message>>,
    pub cancel: CancellationToken,
}

/// What the session that accepted a request promised about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionFacts {
    /// Whether a journal with no record of a run proves the run never arrived.
    pub authoritative_absence: bool,
}

/// What this hub run knows about one host's session, as the action executor needs it.
///
/// `last_seen` is not enough here: it is carried over between frames and set by tracking a
/// host, so it cannot say whether a session ever existed or when it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionState {
    /// A session is live, started at `since`.
    Live {
        /// When the live session started.
        since: Timestamp,
    },
    /// A session existed in this hub run and ended at `at`.
    Lost {
        /// When the last session ended.
        at: Timestamp,
    },
    /// No session has existed in this hub run.
    Never,
}

/// One host as the presence check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Watch {
    pub host: HostId,
    pub last_seen: Option<Timestamp>,
    pub remote_ip: Option<IpAddr>,
    pub raised: Option<Liveness>,
}
