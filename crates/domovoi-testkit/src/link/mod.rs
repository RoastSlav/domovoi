//! A fake agent link and a recording signal sink.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionAnswer, ActionAnswerEntry, ActionAnswers, AgentLink, CheckRecheck, HostAvailability,
    HostReconciler, IngestError, LinkError, Presence, SignalSink, SkillDispatch, SkillOutput,
    ToolOutput,
};
use domovoi_core::{
    ActionId, CheckKey, HostId, JournalEpoch, ReadToolCall, Signal, SkillSummary, TargetKey,
};
use domovoi_proto::messages::{CheckInfo, HostFacts};
use jiff::Timestamp;

use crate::lock;

type ToolHandler =
    Box<dyn Fn(HostId, &ReadToolCall) -> Result<ToolOutput, LinkError> + Send + Sync>;
type SkillHandler =
    Box<dyn Fn(HostId, &SkillDispatch) -> Result<SkillOutput, LinkError> + Send + Sync>;
type CheckHandler = Box<
    dyn Fn(HostId, &CheckKey, Option<&TargetKey>) -> Result<CheckRecheck, LinkError> + Send + Sync,
>;
type StatusHandler = Arc<
    dyn Fn(HostId, &[ActionId], JournalEpoch) -> Result<ActionAnswers, LinkError> + Send + Sync,
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
    /// `action_status`.
    Status {
        /// Target host.
        host: HostId,
        /// The executions asked about.
        ids: Vec<ActionId>,
        /// The epoch the caller believes the journal has.
        expected_epoch: JournalEpoch,
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

/// An [`AgentLink`] with scripted presence, skills, host facts and handlers.
///
/// Every request is recorded. A host that is not [`Presence::Connected`] gets
/// [`LinkError::NotConnected`]; a connected host with no handler for the request kind gets
/// [`LinkError::Rejected`]. Deadlines are recorded but never enforced.
pub struct FakeAgentLink {
    presence: Mutex<BTreeMap<HostId, Presence>>,
    availability: Mutex<BTreeMap<HostId, HostAvailability>>,
    skills: Mutex<BTreeMap<HostId, Vec<SkillSummary>>>,
    facts: Mutex<BTreeMap<HostId, HostFacts>>,
    checks: Mutex<BTreeMap<HostId, Vec<CheckInfo>>>,
    epochs: Mutex<BTreeMap<HostId, JournalEpoch>>,
    answers: Mutex<BTreeMap<HostId, BTreeMap<ActionId, ActionAnswer>>>,
    status: Mutex<Option<StatusHandler>>,
    handlers: Mutex<Handlers>,
    requests: Mutex<Vec<LinkRequest>>,
}

impl FakeAgentLink {
    /// A link where no host is connected, nothing announced and no handlers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            presence: Mutex::new(BTreeMap::new()),
            availability: Mutex::new(BTreeMap::new()),
            skills: Mutex::new(BTreeMap::new()),
            facts: Mutex::new(BTreeMap::new()),
            checks: Mutex::new(BTreeMap::new()),
            epochs: Mutex::new(BTreeMap::new()),
            answers: Mutex::new(BTreeMap::new()),
            status: Mutex::new(None),
            handlers: Mutex::new(Handlers::default()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Sets a host's presence.
    pub fn set_presence(&self, host: HostId, presence: Presence) {
        lock(&self.presence).insert(host, presence);
    }

    /// Sets what `availability` reports for a host, separate from its presence. A test uses
    /// this to say a host is unknown even though the fake knows when it was last seen.
    pub fn set_availability(&self, host: HostId, availability: HostAvailability) {
        lock(&self.availability).insert(host, availability);
    }

    /// Drops a scripted availability, so the host's presence decides again.
    pub fn clear_availability(&self, host: HostId) {
        lock(&self.availability).remove(&host);
    }

    /// Sets the skills a host announces.
    pub fn set_skills(&self, host: HostId, skills: Vec<SkillSummary>) {
        lock(&self.skills).insert(host, skills);
    }

    /// Sets what a host's agent said about its machine, as a handshake would.
    pub fn set_facts(&self, host: HostId, facts: HostFacts) {
        lock(&self.facts).insert(host, facts);
    }

    /// What the host's agent said about its machine, `None` while never announced.
    #[must_use]
    pub fn host_facts(&self, host: HostId) -> Option<HostFacts> {
        lock(&self.facts).get(&host).cloned()
    }

    /// Sets the checks a host's agent announced, as a handshake would.
    pub fn set_checks(&self, host: HostId, checks: Vec<CheckInfo>) {
        lock(&self.checks).insert(host, checks);
    }

    /// The checks the host's agent announced; empty while never announced.
    #[must_use]
    pub fn host_checks(&self, host: HostId) -> Vec<CheckInfo> {
        lock(&self.checks).get(&host).cloned().unwrap_or_default()
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

    /// Sets the journal epoch a host's agent reported.
    pub fn set_journal_epoch(&self, host: HostId, epoch: JournalEpoch) {
        lock(&self.epochs).insert(host, epoch);
    }

    /// Drops a host's journal epoch, so `journal_epoch` answers `None` for it.
    pub fn clear_journal_epoch(&self, host: HostId) {
        lock(&self.epochs).remove(&host);
    }

    /// Scripts what the host's journal says about one run.
    pub fn answer(&self, host: HostId, id: ActionId, answer: ActionAnswer) {
        lock(&self.answers)
            .entry(host)
            .or_default()
            .insert(id, answer);
    }

    /// Answers every `action_status` with `handler`, instead of the scripted answers.
    pub fn on_action_status(
        &self,
        handler: impl Fn(HostId, &[ActionId], JournalEpoch) -> Result<ActionAnswers, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        *lock(&self.status) = Some(Arc::new(handler));
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

    async fn availability(&self, host: HostId) -> HostAvailability {
        if let Some(scripted) = lock(&self.availability).get(&host).cloned() {
            return scripted;
        }
        // The trait's default, spelled out because calling it here would recurse.
        match self.presence(host) {
            Presence::Connected { since, .. } => HostAvailability::Known { since },
            Presence::Disconnected {
                last_seen: Some(since),
            } => HostAvailability::Absent { since },
            Presence::Disconnected { last_seen: None } => HostAvailability::Unknown,
        }
    }

    fn journal_epoch(&self, host: HostId) -> Option<JournalEpoch> {
        lock(&self.epochs).get(&host).copied()
    }

    async fn action_status(
        &self,
        host: HostId,
        ids: Vec<ActionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError> {
        lock(&self.requests).push(LinkRequest::Status {
            host,
            ids: ids.clone(),
            expected_epoch,
            deadline,
        });
        self.connected(host)?;
        if let Some(handler) = lock(&self.status).clone() {
            return handler(host, &ids, expected_epoch);
        }
        let scripted = lock(&self.answers);
        let host_answers = scripted.get(&host);
        let answers = ids
            .into_iter()
            .map(|id| ActionAnswerEntry {
                execution: id,
                answer: host_answers
                    .and_then(|known| known.get(&id))
                    .cloned()
                    .unwrap_or(ActionAnswer::NotFound),
            })
            .collect();
        Ok(ActionAnswers {
            epoch: expected_epoch,
            answers,
        })
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

/// A [`HostReconciler`] that records every host it was asked to reconcile.
#[derive(Debug, Default)]
pub struct RecordingReconciler {
    hosts: Mutex<Vec<HostId>>,
    notify: tokio::sync::Notify,
}

impl RecordingReconciler {
    /// A reconciler that has been asked about nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every host it was asked to reconcile, oldest first.
    #[must_use]
    pub fn hosts(&self) -> Vec<HostId> {
        lock(&self.hosts).clone()
    }

    /// Waits until at least `count` hosts have been recorded and returns the list, or
    /// returns at once when that many already were. No polling and no sleeping: it uses
    /// the `Notify` woken by the next recorded host.
    pub async fn wait_for(&self, count: usize) -> Vec<HostId> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let hosts = lock(&self.hosts).clone();
            if hosts.len() >= count {
                return hosts;
            }
            notified.await;
        }
    }
}

#[async_trait]
impl HostReconciler for RecordingReconciler {
    async fn reconcile_host(&self, host: HostId) {
        lock(&self.hosts).push(host);
        self.notify.notify_waiters();
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
