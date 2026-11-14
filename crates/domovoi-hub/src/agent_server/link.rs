//! [`WsAgentLink`]: the engine's way to reach agents through their live sessions.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    AgentLink, CheckRecheck, Clock, HostAvailability, HostRepo, LinkError, LogLine, Presence,
    SkillDispatch, SkillOutput, ToolOutput,
};
use domovoi_core::{CheckKey, HostId, ReadToolCall, SkillSummary, TargetKey};
use domovoi_proto::messages::{
    GetLogs, RunCheck, RunSkill, RunTool, SkillFailureCode, SkillOutcome, ToolFailureCode,
    ToolOutcome,
};
use domovoi_proto::{Message, PROTOCOL_VERSION};
use jiff::SignedDuration;
use jiff::Timestamp;

use super::compat::{ProtocolState, protocol_state};
use super::registry::{Registry, SessionState};

/// Routes requests to the host's current session. Fails fast with
/// [`LinkError::NotConnected`] when the host has none, and with [`LinkError::Timeout`]
/// once the deadline passes.
pub struct WsAgentLink {
    registry: Arc<Registry>,
    clock: Arc<dyn Clock>,
    hosts: Arc<dyn HostRepo>,
}

impl WsAgentLink {
    pub(crate) fn new(
        registry: Arc<Registry>,
        clock: Arc<dyn Clock>,
        hosts: Arc<dyn HostRepo>,
    ) -> Self {
        Self {
            registry,
            clock,
            hosts,
        }
    }

    /// Why a request found no live session: the hub rejected this agent's protocol, or it
    /// is simply not connected. A prediction that it would be incompatible is not a
    /// finding, so that case stays [`LinkError::NotConnected`].
    async fn not_connected(&self, host: HostId) -> LinkError {
        let report = match self.hosts.live_agent(host).await {
            Ok(Some(agent)) => self.hosts.agent_report(agent.agent).await,
            Ok(None) => return LinkError::NotConnected,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent report");
                return LinkError::NotConnected;
            }
        };
        let report = match report {
            Ok(report) => report,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent report");
                return LinkError::NotConnected;
            }
        };
        match protocol_state(report.as_ref(), PROTOCOL_VERSION) {
            ProtocolState::Incompatible { .. } => LinkError::Incompatible,
            _ => LinkError::NotConnected,
        }
    }

    /// The version the host's agent reported when this hub rejected it, or `None` when
    /// the agent is not rejected. A report that merely predates this hub's protocol is a
    /// prediction, not a finding, so it is `None` too.
    async fn rejected_protocol(&self, host: HostId) -> Option<String> {
        let agent = match self.hosts.live_agent(host).await {
            Ok(Some(agent)) => agent,
            Ok(None) => return None,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent");
                return None;
            }
        };
        let report = match self.hosts.agent_report(agent.agent).await {
            Ok(report) => report,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the host's agent report");
                return None;
            }
        };
        match protocol_state(report.as_ref(), PROTOCOL_VERSION) {
            ProtocolState::Incompatible { reported, .. } => Some(reported.to_string()),
            _ => None,
        }
    }

    async fn request(
        &self,
        host: HostId,
        message: &Message,
        deadline: Timestamp,
    ) -> Result<Message, LinkError> {
        if self.clock.now() >= deadline {
            return Err(LinkError::Timeout);
        }
        let (id, answer) = match self.registry.dispatch(host, message) {
            Ok(pair) => pair,
            Err(LinkError::NotConnected) => return Err(self.not_connected(host).await),
            Err(err) => return Err(err),
        };
        let reply = tokio::select! {
            reply = answer => reply.map_err(|_| {
                LinkError::Transport("the agent disconnected before answering".to_owned())
            }),
            () = self.clock.sleep_until(deadline) => Err(LinkError::Timeout),
        };
        self.registry.forget(host, id);
        reply
    }

    /// The recent log lines of the agent on `host`: waits 10 s for a reply.
    /// The window may not fit one frame, so the agent answers in chunks and the
    /// hub asks again with `offset` advanced, until the reply says it has no
    /// more lines. An agent that predates `get_logs` replies `unsupported`,
    /// reported as [`LinkError::Rejected`] telling the person to update the agent.
    pub async fn logs(
        &self,
        host: HostId,
        lines: u16,
        min_level: String,
    ) -> Result<Vec<LogLine>, LinkError> {
        let deadline = self.clock.now() + SignedDuration::from_secs(10);
        let window = usize::from(lines);
        let mut got: Vec<LogLine> = Vec::new();
        loop {
            let get = GetLogs {
                lines,
                min_level: min_level.clone(),
                // The window is at most `lines`, which fits in a `u16`.
                offset: got.len().min(window) as u16,
                deadline,
            };
            let reply = match self.request(host, &Message::GetLogs(get), deadline).await? {
                Message::Logs(reply) => reply,
                Message::Unsupported(_) => {
                    if !got.is_empty() {
                        return Ok(got);
                    }
                    return Err(LinkError::Rejected(
                        "the agent is too old to send logs; update it".to_owned(),
                    ));
                }
                Message::Error(err) => {
                    // A full request queue answers with a busy error right away, so
                    // the caller does not wait out the deadline for it.
                    return Err(LinkError::Rejected(err.message));
                }
                other => return Err(unexpected(&other)),
            };
            let more = reply.has_more;
            let empty = reply.lines.is_empty();
            got.extend(reply.lines);
            if got.len() >= window || !more || empty {
                break;
            }
        }
        got.truncate(window);
        Ok(got)
    }
}

fn unexpected(message: &Message) -> LinkError {
    LinkError::Rejected(format!("the agent answered with {}", message.kind()))
}

#[async_trait]
impl AgentLink for WsAgentLink {
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        let message = Message::RunTool(RunTool { call, deadline });
        match self.request(host, &message, deadline).await? {
            Message::ToolResult(result) => match result.outcome {
                ToolOutcome::Ok(out) => Ok(out.into()),
                ToolOutcome::Err(f) if f.code == ToolFailureCode::Timeout => {
                    Err(LinkError::Timeout)
                }
                ToolOutcome::Err(f) => {
                    Err(LinkError::Rejected(format!("{:?}: {}", f.code, f.message)))
                }
            },
            other => Err(unexpected(&other)),
        }
    }

    async fn run_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        let message = Message::RunSkill(RunSkill::from_dispatch(&req, deadline));
        match self.request(host, &message, deadline).await? {
            Message::SkillResult(result) => match result.outcome {
                SkillOutcome::Ok(out) => Ok(out.into()),
                SkillOutcome::Err(f) if f.code == SkillFailureCode::Timeout => {
                    Err(LinkError::Timeout)
                }
                SkillOutcome::Err(f) => {
                    Err(LinkError::Rejected(format!("{:?}: {}", f.code, f.message)))
                }
            },
            other => Err(unexpected(&other)),
        }
    }

    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        let message = Message::RunCheck(RunCheck {
            check,
            target: target.clone(),
            deadline,
        });
        match self.request(host, &message, deadline).await? {
            Message::CheckResult(result) => {
                let mut observations = result.observations;
                if observations.is_empty() {
                    return Err(LinkError::Rejected(
                        "the check reported no observation".to_owned(),
                    ));
                }
                let at = observations
                    .iter()
                    .position(|o| o.target == target)
                    .unwrap_or(0);
                Ok(observations.swap_remove(at).into())
            }
            other => Err(unexpected(&other)),
        }
    }

    fn presence(&self, host: HostId) -> Presence {
        self.registry.presence(host)
    }

    async fn availability(&self, host: HostId) -> HostAvailability {
        // A live session is the answer whether or not an older report says otherwise.
        let state = self.registry.session_state(host);
        if let SessionState::Live { since } = state {
            return HostAvailability::Known { since };
        }
        // A host this hub rejected is reported as incompatible even after its session
        // ended, so an approval waits for an upgrade rather than for a reconnect.
        if let Some(reported) = self.rejected_protocol(host).await {
            return HostAvailability::Incompatible { reported };
        }
        match state {
            SessionState::Lost { at } => HostAvailability::Absent { since: at },
            // A live session was answered above; a host never seen in this hub run is
            // unknown until it connects.
            SessionState::Live { .. } | SessionState::Never => HostAvailability::Unknown,
        }
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.registry.skills(host)
    }
}
