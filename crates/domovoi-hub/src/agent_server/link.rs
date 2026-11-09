//! [`WsAgentLink`]: the engine's way to reach agents through their live sessions.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    AgentLink, CheckRecheck, Clock, LinkError, LogLine, Presence, SkillDispatch, SkillOutput,
    ToolOutput,
};
use domovoi_core::{CheckKey, HostId, ReadToolCall, SkillSummary, TargetKey};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    GetLogs, RunCheck, RunSkill, RunTool, SkillFailureCode, SkillOutcome, ToolFailureCode,
    ToolOutcome,
};
use jiff::SignedDuration;
use jiff::Timestamp;

use super::registry::Registry;

/// Routes requests to the host's current session. Fails fast with
/// [`LinkError::NotConnected`] when the host has none, and with [`LinkError::Timeout`]
/// once the deadline passes.
pub struct WsAgentLink {
    registry: Arc<Registry>,
    clock: Arc<dyn Clock>,
}

impl WsAgentLink {
    pub(crate) fn new(registry: Arc<Registry>, clock: Arc<dyn Clock>) -> Self {
        Self { registry, clock }
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
        let (id, answer) = self.registry.dispatch(host, message)?;
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

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.registry.skills(host)
    }
}
