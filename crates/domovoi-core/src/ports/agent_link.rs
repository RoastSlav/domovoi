//! The hub side of an agent connection, and where signals enter the hub.

use async_trait::async_trait;
use jiff::Timestamp;

use crate::action::PlanHash;
use crate::id::{ActionId, HostId};
use crate::signal::{CheckKey, Evidence, Signal, SignalStatus, TargetKey};
use crate::skill::{SkillName, SkillSummary, ValidatedArgs};
use crate::triage::ReadToolCall;

/// What a read tool returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// The output text, already redacted by the agent.
    pub text: String,
    /// Whether the agent cut the output at its cap.
    pub truncated: bool,
}

/// An approved action sent to an agent for execution.
///
/// Only the action executor builds one, after the action reached `Dispatched` with a
/// matching plan hash. The agent re-validates `args` and `plan_hash` against its own
/// manifest before running anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDispatch {
    /// The action being executed.
    pub action: ActionId,
    /// The skill to run.
    pub skill: SkillName,
    /// The validated arguments.
    pub args: ValidatedArgs,
    /// The plan hash the approval bound to.
    pub plan_hash: PlanHash,
}

/// What a skill run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillOutput {
    /// Exit code, or `None` when killed.
    pub exit_code: Option<i32>,
    /// Standard output, redacted and capped.
    pub stdout: String,
    /// Standard error, redacted and capped.
    pub stderr: String,
    /// Whether either stream was cut.
    pub truncated: bool,
    /// Wall-clock run time in milliseconds.
    pub duration_ms: u64,
    /// SHA-256 of the argv the agent ran, for the audit log.
    pub argv_hash: [u8; 32],
}

/// The result of re-running one check on demand.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckRecheck {
    /// Whether the condition still holds.
    pub status: SignalStatus,
    /// One-line description.
    pub summary: String,
    /// Fresh evidence.
    pub evidence: Evidence,
    /// When the check ran.
    pub observed_at: Timestamp,
}

/// Whether an agent is connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    /// Connected now.
    Connected {
        /// Since when.
        since: Timestamp,
        /// Agent version from the handshake.
        version: String,
    },
    /// Not connected.
    Disconnected {
        /// When it was last connected, if ever.
        last_seen: Option<Timestamp>,
    },
}

/// Where a host's agent stands, as the action executor needs to know before it dispatches.
///
/// Unlike [`Presence`], this separates a host that has never been seen (**Unknown**) from
/// one whose agent connected and then went away (**Absent**), and reports a host whose
/// agent speaks another protocol version (**Incompatible**) instead of treating it as
/// merely offline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAvailability {
    /// The agent is connected now, since `since`.
    Known {
        /// When this session started.
        since: Timestamp,
    },
    /// The host has no agent session and never had one in this hub run.
    Unknown,
    /// The host's agent was connected and has been gone since `since`.
    Absent {
        /// When the last session ended.
        since: Timestamp,
    },
    /// The hub rejected the host's agent because it speaks another protocol version.
    Incompatible {
        /// The version the agent reported.
        reported: String,
    },
}

/// Why a request to an agent failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// The host has no connected agent.
    #[error("agent not connected")]
    NotConnected,
    /// The host's agent speaks another protocol version than this hub. Work on the host is
    /// unavailable until the agent is upgraded. Reason code: `agent_incompatible`.
    #[error("agent_incompatible: the agent speaks another protocol version; upgrade it")]
    Incompatible,
    /// No answer before the deadline.
    #[error("timed out")]
    Timeout,
    /// The agent refused the request, for example after local re-validation.
    #[error("rejected by agent: {0}")]
    Rejected(String),
    /// The connection failed mid-request.
    #[error("transport error: {0}")]
    Transport(String),
}

/// Sends requests to connected agents. Every method fails fast with
/// [`LinkError::NotConnected`] when the host has no connected agent, with
/// [`LinkError::Incompatible`] when this hub rejected the agent's protocol, and with
/// [`LinkError::Timeout`] once `deadline` passes.
#[async_trait]
pub trait AgentLink: Send + Sync {
    /// Runs one read tool on `host`.
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError>;

    /// Runs one approved skill on `host`; see [`SkillDispatch`].
    async fn run_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError>;

    /// Re-runs one check on `host`, for verification after an action.
    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError>;

    /// Current connection state of `host`'s agent.
    fn presence(&self, host: HostId) -> Presence;

    /// Whether the host's agent is reachable, was reachable and left, was never seen, or
    /// was rejected. Links that only know [`Presence`] get this default, which cannot
    /// tell a host that never connected from one that disconnected.
    async fn availability(&self, host: HostId) -> HostAvailability {
        match self.presence(host) {
            Presence::Connected { since, .. } => HostAvailability::Known { since },
            Presence::Disconnected {
                last_seen: Some(since),
            } => HostAvailability::Absent { since },
            Presence::Disconnected { last_seen: None } => HostAvailability::Unknown,
        }
    }

    /// Skills the agent announced on its last handshake; empty when never connected.
    fn skills(&self, host: HostId) -> Vec<SkillSummary>;
}

/// Why signals were not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IngestError {
    /// The batch was refused, for example for an unknown host.
    #[error("rejected: {0}")]
    Rejected(String),
    /// The hub cannot accept signals right now; the caller should retry or spool.
    #[error("unavailable: {0}")]
    Unavailable(String),
}

/// Where agents, webhooks and probes hand signals to the hub.
#[async_trait]
pub trait SignalSink: Send + Sync {
    /// Accepts a batch, in order. Either the whole batch is accepted or none of it.
    async fn submit(&self, signals: Vec<Signal>) -> Result<(), IngestError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A link that only knows presence, so it exercises the default `availability`.
    struct PresenceOnly(Presence);

    #[async_trait]
    impl AgentLink for PresenceOnly {
        async fn run_tool(
            &self,
            _host: HostId,
            _call: ReadToolCall,
            _deadline: Timestamp,
        ) -> Result<ToolOutput, LinkError> {
            Err(LinkError::NotConnected)
        }

        async fn run_skill(
            &self,
            _host: HostId,
            _req: SkillDispatch,
            _deadline: Timestamp,
        ) -> Result<SkillOutput, LinkError> {
            Err(LinkError::NotConnected)
        }

        async fn run_check(
            &self,
            _host: HostId,
            _check: CheckKey,
            _target: Option<TargetKey>,
            _deadline: Timestamp,
        ) -> Result<CheckRecheck, LinkError> {
            Err(LinkError::NotConnected)
        }

        fn presence(&self, _host: HostId) -> Presence {
            self.0.clone()
        }

        fn skills(&self, _host: HostId) -> Vec<SkillSummary> {
            Vec::new()
        }
    }

    #[test]
    fn availability_default_follows_presence() {
        futures::executor::block_on(async {
            let host = HostId::new();
            let since = Timestamp::constant(1_700_000_000, 0);

            let connected = PresenceOnly(Presence::Connected {
                since,
                version: "0.1.0".to_owned(),
            });
            assert_eq!(
                connected.availability(host).await,
                HostAvailability::Known { since },
            );

            let left = PresenceOnly(Presence::Disconnected {
                last_seen: Some(since),
            });
            assert_eq!(
                left.availability(host).await,
                HostAvailability::Absent { since },
            );

            let never = PresenceOnly(Presence::Disconnected { last_seen: None });
            assert_eq!(never.availability(host).await, HostAvailability::Unknown);
        });
    }
}
