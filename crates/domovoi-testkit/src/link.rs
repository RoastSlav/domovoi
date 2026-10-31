//! A fake agent link and a recording signal sink.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::ports::{
    AgentLink, CheckRecheck, IngestError, LinkError, Presence, SignalSink, SkillDispatch,
    SkillOutput, ToolOutput,
};
use domovoi_core::{CheckKey, HostId, ReadToolCall, Signal, SkillSummary, TargetKey};
use jiff::Timestamp;

use crate::lock;

type ToolHandler =
    Box<dyn Fn(HostId, &ReadToolCall) -> Result<ToolOutput, LinkError> + Send + Sync>;
type SkillHandler =
    Box<dyn Fn(HostId, &SkillDispatch) -> Result<SkillOutput, LinkError> + Send + Sync>;
type CheckHandler = Box<
    dyn Fn(HostId, &CheckKey, Option<&TargetKey>) -> Result<CheckRecheck, LinkError> + Send + Sync,
>;

/// One request made through a [`FakeAgentLink`].
#[derive(Debug, Clone, PartialEq)]
pub enum LinkRequest {
    /// `run_tool`.
    Tool {
        /// Target host.
        host: HostId,
        /// The call.
        call: ReadToolCall,
        /// The deadline passed.
        deadline: Timestamp,
    },
    /// `run_skill`.
    Skill {
        /// Target host.
        host: HostId,
        /// The dispatch.
        dispatch: SkillDispatch,
        /// The deadline passed.
        deadline: Timestamp,
    },
    /// `run_check`.
    Check {
        /// Target host.
        host: HostId,
        /// The check.
        check: CheckKey,
        /// The target.
        target: Option<TargetKey>,
        /// The deadline passed.
        deadline: Timestamp,
    },
}

#[derive(Default)]
struct Handlers {
    tool: Option<ToolHandler>,
    skill: Option<SkillHandler>,
    check: Option<CheckHandler>,
}

/// An [`AgentLink`] with scripted presence, skills and handlers.
///
/// Every request is recorded. A host that is not [`Presence::Connected`] gets
/// [`LinkError::NotConnected`]; a connected host with no handler for the request kind gets
/// [`LinkError::Rejected`]. Deadlines are recorded but never enforced.
pub struct FakeAgentLink {
    presence: Mutex<BTreeMap<HostId, Presence>>,
    skills: Mutex<BTreeMap<HostId, Vec<SkillSummary>>>,
    handlers: Mutex<Handlers>,
    requests: Mutex<Vec<LinkRequest>>,
}

impl FakeAgentLink {
    /// A link where no host is connected.
    #[must_use]
    pub fn new() -> Self {
        Self {
            presence: Mutex::new(BTreeMap::new()),
            skills: Mutex::new(BTreeMap::new()),
            handlers: Mutex::new(Handlers::default()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Sets a host's presence.
    pub fn set_presence(&self, host: HostId, presence: Presence) {
        lock(&self.presence).insert(host, presence);
    }

    /// Sets the skills a host announces.
    pub fn set_skills(&self, host: HostId, skills: Vec<SkillSummary>) {
        lock(&self.skills).insert(host, skills);
    }

    /// Answers `run_tool` with `handler`.
    pub fn on_tool(
        &self,
        handler: impl Fn(HostId, &ReadToolCall) -> Result<ToolOutput, LinkError> + Send + Sync + 'static,
    ) {
        lock(&self.handlers).tool = Some(Box::new(handler));
    }

    /// Answers `run_skill` with `handler`.
    pub fn on_skill(
        &self,
        handler: impl Fn(HostId, &SkillDispatch) -> Result<SkillOutput, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        lock(&self.handlers).skill = Some(Box::new(handler));
    }

    /// Answers `run_check` with `handler`.
    pub fn on_check(
        &self,
        handler: impl Fn(HostId, &CheckKey, Option<&TargetKey>) -> Result<CheckRecheck, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        lock(&self.handlers).check = Some(Box::new(handler));
    }

    /// Every request so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<LinkRequest> {
        lock(&self.requests).clone()
    }

    fn connected(&self, host: HostId) -> Result<(), LinkError> {
        match self.presence(host) {
            Presence::Connected { .. } => Ok(()),
            Presence::Disconnected { .. } => Err(LinkError::NotConnected),
        }
    }
}

impl Default for FakeAgentLink {
    fn default() -> Self {
        Self::new()
    }
}

fn no_handler() -> LinkError {
    LinkError::Rejected("no handler registered".to_owned())
}

#[async_trait]
impl AgentLink for FakeAgentLink {
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        lock(&self.requests).push(LinkRequest::Tool {
            host,
            call: call.clone(),
            deadline,
        });
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.tool.as_ref().ok_or_else(no_handler)?;
        handler(host, &call)
    }

    async fn run_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        lock(&self.requests).push(LinkRequest::Skill {
            host,
            dispatch: req.clone(),
            deadline,
        });
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.skill.as_ref().ok_or_else(no_handler)?;
        handler(host, &req)
    }

    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        lock(&self.requests).push(LinkRequest::Check {
            host,
            check: check.clone(),
            target: target.clone(),
            deadline,
        });
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.check.as_ref().ok_or_else(no_handler)?;
        handler(host, &check, target.as_ref())
    }

    fn presence(&self, host: HostId) -> Presence {
        lock(&self.presence)
            .get(&host)
            .cloned()
            .unwrap_or(Presence::Disconnected { last_seen: None })
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        lock(&self.skills).get(&host).cloned().unwrap_or_default()
    }
}

/// A [`SignalSink`] that records every accepted batch.
#[derive(Debug, Default)]
pub struct RecordingSignalSink {
    batches: Mutex<Vec<Vec<Signal>>>,
    failures: Mutex<VecDeque<IngestError>>,
}

impl RecordingSignalSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next submit fail with `error`; the batch is not recorded.
    pub fn fail_next(&self, error: IngestError) {
        lock(&self.failures).push_back(error);
    }

    /// Every accepted batch, oldest first.
    #[must_use]
    pub fn batches(&self) -> Vec<Vec<Signal>> {
        lock(&self.batches).clone()
    }
}

#[async_trait]
impl SignalSink for RecordingSignalSink {
    async fn submit(&self, signals: Vec<Signal>) -> Result<(), IngestError> {
        if let Some(error) = lock(&self.failures).pop_front() {
            return Err(error);
        }
        lock(&self.batches).push(signals);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_agent_link_checks_presence_then_handlers() {
        let link = FakeAgentLink::new();
        let host = HostId::new();
        let at = Timestamp::constant(1_700_000_000, 0);
        let call = ReadToolCall::HostOverview;
        assert_eq!(
            link.run_tool(host, call.clone(), at).await,
            Err(LinkError::NotConnected)
        );

        link.set_presence(
            host,
            Presence::Connected {
                since: at,
                version: "0.1.0".to_owned(),
            },
        );
        assert!(matches!(
            link.run_tool(host, call.clone(), at).await,
            Err(LinkError::Rejected(_))
        ));

        link.on_tool(|_, _| {
            Ok(ToolOutput {
                text: "up 3 days".to_owned(),
                truncated: false,
            })
        });
        let out = link.run_tool(host, call, at).await.unwrap();
        assert_eq!(out.text, "up 3 days");
        assert_eq!(link.requests().len(), 3);
        assert!(link.skills(host).is_empty());
    }
}
